// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/model.c, Mod_LoadBrushModel, Mod_LoadVertexes, Mod_LoadEdges, Mod_LoadSurfedges, Mod_LoadTextures, Mod_LoadLighting, Mod_LoadPlanes, Mod_LoadTexinfo, CalcSurfaceExtents, Mod_LoadFaces, Mod_LoadMarksurfaces, Mod_LoadVisibility, Mod_LoadLeafs, Mod_LoadNodes, Mod_LoadClipnodes, Mod_LoadEntities, Mod_LoadSubmodels, Mod_MakeHull0, RadiusFromBounds, Mod_DecompressVis, Mod_LeafPVS
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/gl_model.c, CalcSurfaceExtents, Mod_LoadLeafs
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/world.c, SV_HullForEntity

//! Reads compiled maps: BSP29 as id's engine loads it, the two BSP2 variants, and `.lit`
//! coloured light (`docs/spec/bsp.md`, BSP-1 to BSP-19).
//!
//! [`Bsp::read`] is id's `Mod_LoadBrushModel` and the loaders it calls, in the same order,
//! with the bounds and index checks id's engine leaves out. Its caller's [`BspLoader`] picks
//! whose rules apply where loaders differ: WinQuake's, GLQuake's, or by default the FitzQuake
//! lineage's as QuakeSpasm has them (BSP-20, ADR-0012). It rejects malformed input with
//! a [`BspError`], never a panic. Textures, lighting, visibility and the entity text borrow
//! the file's bytes. BSP2 and the FitzQuake-lineage rules follow QuakeSpasm's loader, cited for
//! description only; nothing is ported from it. [`lit`] reads `.lit` files and [`ent`] names `.ent` files, and
//! [`replace_entities`] writes a BSP with a new entity lump: the two ways to edit the
//! entities of a shipped map that has no `.map` source.

pub mod ent;
pub mod lit;

#[cfg(test)]
mod fitz_tests;
#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tests;

use core::fmt;

pub use ent::{BSP_ENT_EXTENSION, ent_file_names};
pub use lit::{BSP_LIT_HEADER_SIZE, BSP_LIT_MAGIC, BSP_LIT_VERSION, Lit, LitError};

/// The version of a BSP29 file, id's `BSPVERSION` (BSP-1.2).
pub const BSP_VERSION: i32 = 29;
/// The version field of a `2PSB` file, QuakeSpasm's `BSP2VERSION_2PSB` (BSP-17.1).
pub const BSP_2PSB_VERSION: i32 = i32::from_le_bytes(*b"2PSB");
/// The version field of a `BSP2` file, QuakeSpasm's `BSP2VERSION_BSP2` (BSP-17.1).
pub const BSP_BSP2_VERSION: i32 = i32::from_le_bytes(*b"BSP2");

/// The number of lumps, id's `HEADER_LUMPS` (BSP-1.1).
pub const BSP_HEADER_LUMPS: usize = 15;
/// Bytes in the header, id's `sizeof(dheader_t)` (BSP-1.1).
pub const BSP_HEADER_SIZE: usize = 124;

/// `LUMP_ENTITIES`.
pub const BSP_LUMP_ENTITIES: usize = 0;
/// `LUMP_PLANES`.
pub const BSP_LUMP_PLANES: usize = 1;
/// `LUMP_TEXTURES`.
pub const BSP_LUMP_TEXTURES: usize = 2;
/// `LUMP_VERTEXES`.
pub const BSP_LUMP_VERTEXES: usize = 3;
/// `LUMP_VISIBILITY`.
pub const BSP_LUMP_VISIBILITY: usize = 4;
/// `LUMP_NODES`.
pub const BSP_LUMP_NODES: usize = 5;
/// `LUMP_TEXINFO`.
pub const BSP_LUMP_TEXINFO: usize = 6;
/// `LUMP_FACES`.
pub const BSP_LUMP_FACES: usize = 7;
/// `LUMP_LIGHTING`.
pub const BSP_LUMP_LIGHTING: usize = 8;
/// `LUMP_CLIPNODES`.
pub const BSP_LUMP_CLIPNODES: usize = 9;
/// `LUMP_LEAFS`.
pub const BSP_LUMP_LEAFS: usize = 10;
/// `LUMP_MARKSURFACES`.
pub const BSP_LUMP_MARKSURFACES: usize = 11;
/// `LUMP_EDGES`.
pub const BSP_LUMP_EDGES: usize = 12;
/// `LUMP_SURFEDGES`.
pub const BSP_LUMP_SURFEDGES: usize = 13;
/// `LUMP_MODELS`.
pub const BSP_LUMP_MODELS: usize = 14;

/// Bytes in a plane, `sizeof(dplane_t)` (BSP-3.1).
pub const BSP_PLANE_SIZE: usize = 20;
/// Plane type `PLANE_X`.
pub const BSP_PLANE_X: i32 = 0;
/// Plane type `PLANE_Y`.
pub const BSP_PLANE_Y: i32 = 1;
/// Plane type `PLANE_Z`.
pub const BSP_PLANE_Z: i32 = 2;
/// Plane type `PLANE_ANYX`.
pub const BSP_PLANE_ANYX: i32 = 3;
/// Plane type `PLANE_ANYY`.
pub const BSP_PLANE_ANYY: i32 = 4;
/// Plane type `PLANE_ANYZ`.
pub const BSP_PLANE_ANYZ: i32 = 5;
/// Bytes in a texture's header, `sizeof(miptex_t)` (BSP-4.3).
pub const BSP_MIPTEX_HEADER_SIZE: usize = 40;
/// Mip levels per texture, `MIPLEVELS` (BSP-4.5).
pub const BSP_MIPLEVELS: usize = 4;
/// Units of an animation frame, `ANIM_CYCLE` (BSP-4.9).
pub const BSP_ANIM_CYCLE: i32 = 2;
/// Bytes in a vertex (BSP-5.1).
pub const BSP_VERTEX_SIZE: usize = 12;
/// The leaves the engine's visibility row holds, `MAX_MAP_LEAFS` (BSP-6).
pub const BSP_MAX_MAP_LEAFS: usize = 8192;
/// Bytes in a BSP29 node, `sizeof(dnode_t)` (BSP-7.1).
pub const BSP_NODE_SIZE: usize = 24;
/// Bytes in a texinfo, `sizeof(texinfo_t)` (BSP-8.1).
pub const BSP_TEXINFO_SIZE: usize = 40;
/// The texinfo flag for sky and liquids, `TEX_SPECIAL` (BSP-8.5).
pub const BSP_TEX_SPECIAL: i32 = 1;
/// Bytes in a BSP29 face, `sizeof(dface_t)` (BSP-9.1).
pub const BSP_FACE_SIZE: usize = 20;
/// Light styles per face, `MAXLIGHTMAPS` (BSP-9.6).
pub const BSP_MAXLIGHTMAPS: usize = 4;
/// The largest extent of a face without `TEX_SPECIAL` in WinQuake (BSP-9.4).
pub const BSP_MAX_SURFACE_EXTENT: i32 = 256;
/// The largest extent of a face without `TEX_SPECIAL` in GLQuake (BSP-9.17).
pub const BSP_GL_MAX_SURFACE_EXTENT: i32 = 512;
/// The largest extent of a face without `TEX_SPECIAL` in the FitzQuake lineage (BSP-20.10).
pub const BSP_FITZ_MAX_SURFACE_EXTENT: i32 = 2000;
/// The most leaves a BSP29 file may have in the FitzQuake lineage (BSP-20.13).
pub const BSP_FITZ_MAX_LEAFS: usize = 32767;
/// The texinfo flag the FitzQuake lineage sets when its texture is missing, QuakeSpasm's
/// `TEX_MISSING` (BSP-20.7).
pub const BSP_TEX_MISSING: i32 = 2;
/// Texels per lightmap sample (BSP-9.7).
pub const BSP_LIGHTMAP_SCALE: i32 = 16;
/// Bytes in a BSP29 clipnode, `sizeof(dclipnode_t)` (BSP-11.1).
pub const BSP_CLIPNODE_SIZE: usize = 8;
/// Hulls per model, `MAX_MAP_HULLS` (BSP-11, BSP-15.1).
pub const BSP_MAX_MAP_HULLS: usize = 4;
/// Hull 1's box, the player's size (BSP-11.3).
pub const BSP_HULL1_MINS: [f32; 3] = [-16.0, -16.0, -24.0];
/// Hull 1's box, the player's size (BSP-11.3).
pub const BSP_HULL1_MAXS: [f32; 3] = [16.0, 16.0, 32.0];
/// Hull 2's box (BSP-11.3).
pub const BSP_HULL2_MINS: [f32; 3] = [-32.0, -32.0, -24.0];
/// Hull 2's box (BSP-11.3).
pub const BSP_HULL2_MAXS: [f32; 3] = [32.0, 32.0, 64.0];
/// Bytes in a BSP29 leaf, `sizeof(dleaf_t)` (BSP-12.1).
pub const BSP_LEAF_SIZE: usize = 28;
/// `CONTENTS_EMPTY`.
pub const BSP_CONTENTS_EMPTY: i32 = -1;
/// `CONTENTS_SOLID`.
pub const BSP_CONTENTS_SOLID: i32 = -2;
/// `CONTENTS_WATER`.
pub const BSP_CONTENTS_WATER: i32 = -3;
/// `CONTENTS_SLIME`.
pub const BSP_CONTENTS_SLIME: i32 = -4;
/// `CONTENTS_LAVA`.
pub const BSP_CONTENTS_LAVA: i32 = -5;
/// `CONTENTS_SKY`.
pub const BSP_CONTENTS_SKY: i32 = -6;
/// Ambient sounds per leaf, `NUM_AMBIENTS` (BSP-12.4).
pub const BSP_NUM_AMBIENTS: usize = 4;
/// Bytes in a BSP29 edge, `sizeof(dedge_t)` (BSP-14.1).
pub const BSP_EDGE_SIZE: usize = 4;
/// Bytes in a surfedge (BSP-14.2).
pub const BSP_SURFEDGE_SIZE: usize = 4;
/// Bytes in a model, `sizeof(dmodel_t)` (BSP-15.1).
pub const BSP_MODEL_SIZE: usize = 64;

/// Bytes in a `2PSB` node, QuakeSpasm's `dl1node_t` (BSP-17).
pub const BSP_2PSB_NODE_SIZE: usize = 32;
/// Bytes in a `BSP2` node, QuakeSpasm's `dl2node_t` (BSP-17).
pub const BSP_BSP2_NODE_SIZE: usize = 44;
/// Bytes in a `2PSB` leaf, QuakeSpasm's `dl1leaf_t` (BSP-17).
pub const BSP_2PSB_LEAF_SIZE: usize = 32;
/// Bytes in a `BSP2` leaf, QuakeSpasm's `dl2leaf_t` (BSP-17).
pub const BSP_BSP2_LEAF_SIZE: usize = 44;
/// Bytes in a BSP2 clipnode, QuakeSpasm's `dlclipnode_t` (BSP-17).
pub const BSP_BSP2_CLIPNODE_SIZE: usize = 12;
/// Bytes in a BSP2 edge, QuakeSpasm's `dledge_t` (BSP-17).
pub const BSP_BSP2_EDGE_SIZE: usize = 8;
/// Bytes in a BSP2 face, QuakeSpasm's `dlface_t` (BSP-17).
pub const BSP_BSP2_FACE_SIZE: usize = 28;
/// Bytes in a BSP2 marksurface (BSP-17).
pub const BSP_BSP2_MARKSURFACE_SIZE: usize = 4;

/// The compiler limits of BSP-16, `MAX_MAP_*` in id's `bspfile.h`; the engine checks none.
pub mod limits {
    /// `MAX_MAP_MODELS`.
    pub const BSP_MAX_MAP_MODELS: usize = 256;
    /// `MAX_MAP_BRUSHES`.
    pub const BSP_MAX_MAP_BRUSHES: usize = 4096;
    /// `MAX_MAP_ENTITIES`.
    pub const BSP_MAX_MAP_ENTITIES: usize = 1024;
    /// `MAX_MAP_ENTSTRING`.
    pub const BSP_MAX_MAP_ENTSTRING: usize = 65536;
    /// `MAX_MAP_PLANES`.
    pub const BSP_MAX_MAP_PLANES: usize = 32767;
    /// `MAX_MAP_NODES`.
    pub const BSP_MAX_MAP_NODES: usize = 32767;
    /// `MAX_MAP_CLIPNODES`.
    pub const BSP_MAX_MAP_CLIPNODES: usize = 32767;
    /// `MAX_MAP_VERTS`.
    pub const BSP_MAX_MAP_VERTS: usize = 65535;
    /// `MAX_MAP_FACES`.
    pub const BSP_MAX_MAP_FACES: usize = 65535;
    /// `MAX_MAP_MARKSURFACES`.
    pub const BSP_MAX_MAP_MARKSURFACES: usize = 65535;
    /// `MAX_MAP_TEXINFO`.
    pub const BSP_MAX_MAP_TEXINFO: usize = 4096;
    /// `MAX_MAP_EDGES`.
    pub const BSP_MAX_MAP_EDGES: usize = 256_000;
    /// `MAX_MAP_SURFEDGES`.
    pub const BSP_MAX_MAP_SURFEDGES: usize = 512_000;
    /// `MAX_MAP_TEXTURES`.
    pub const BSP_MAX_MAP_TEXTURES: usize = 512;
    /// `MAX_MAP_MIPTEX`.
    pub const BSP_MAX_MAP_MIPTEX: usize = 0x20_0000;
    /// `MAX_MAP_LIGHTING`.
    pub const BSP_MAX_MAP_LIGHTING: usize = 0x10_0000;
    /// `MAX_MAP_VISIBILITY`.
    pub const BSP_MAX_MAP_VISIBILITY: usize = 0x10_0000;
    /// `MAX_KEY`.
    pub const BSP_MAX_KEY: usize = 32;
    /// `MAX_VALUE`.
    pub const BSP_MAX_VALUE: usize = 1024;
}

/// Face flag `SURF_PLANEBACK`: the face is on its plane's back (BSP-9.1).
pub const SURF_PLANEBACK: u32 = 2;
/// Face flag `SURF_DRAWSKY` (BSP-9.5).
pub const SURF_DRAWSKY: u32 = 4;
/// Face flag `SURF_DRAWTURB` (BSP-9.5).
pub const SURF_DRAWTURB: u32 = 0x10;
/// Face flag `SURF_DRAWTILED` (BSP-9.5).
pub const SURF_DRAWTILED: u32 = 0x20;
/// Face flag `SURF_UNDERWATER`, GLQuake's only: a leaf that is not empty lists the face
/// (BSP-12.6).
pub const SURF_UNDERWATER: u32 = 0x80;
/// Face flag `SURF_NOTEXTURE`, the FitzQuake lineage's: its texinfo has
/// [`BSP_TEX_MISSING`] (BSP-20.11).
pub const SURF_NOTEXTURE: u32 = 0x100;
/// Face flag `SURF_DRAWFENCE`, the FitzQuake lineage's: a `{` texture (BSP-20.11).
pub const SURF_DRAWFENCE: u32 = 0x200;
/// Face flag `SURF_DRAWLAVA`, the FitzQuake lineage's (BSP-20.11).
pub const SURF_DRAWLAVA: u32 = 0x400;
/// Face flag `SURF_DRAWSLIME`, the FitzQuake lineage's (BSP-20.11).
pub const SURF_DRAWSLIME: u32 = 0x800;
/// Face flag `SURF_DRAWTELE`, the FitzQuake lineage's (BSP-20.11).
pub const SURF_DRAWTELE: u32 = 0x1000;
/// Face flag `SURF_DRAWWATER`, the FitzQuake lineage's (BSP-20.11).
pub const SURF_DRAWWATER: u32 = 0x2000;

/// Which format a file is (BSP-1.2, BSP-17.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BspVersion {
    /// Version 29, id's.
    Bsp29,
    /// `2PSB`: 32-bit fields, 16-bit bounds.
    Bsp2Psb,
    /// `BSP2`: 32-bit fields, float bounds.
    Bsp2,
}

/// Whose loader [`Bsp::read`] follows where loaders differ (BSP-9.17, BSP-12.6, BSP-20).
///
/// The default is [`BspLoader::FitzQuake`], the rules of the engines modders play on
/// (ADR-0012); [`BspLoader::WinQuake`] and [`BspLoader::GlQuake`] answer exactly whether id's
/// own engines load a map.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[expect(
    clippy::enum_variant_names,
    reason = "spec BSP-20.1: each variant is the engine's own name"
)]
pub enum BspLoader {
    /// WinQuake's `WinQuake/model.c`: an extent over 256 fails (BSP-9.4), and no face has
    /// [`SURF_UNDERWATER`].
    WinQuake,
    /// GLQuake's `WinQuake/gl_model.c`: an extent over 512 fails (BSP-9.17), and a face a
    /// leaf that is not empty lists has [`SURF_UNDERWATER`] (BSP-12.6).
    GlQuake,
    /// The FitzQuake lineage (FitzQuake, QuakeSpasm, Ironwail, vkQuake), as QuakeSpasm's
    /// `Quake/gl_model.c` loads a map: BSP-20 says where it differs from GLQuake.
    #[default]
    FitzQuake,
}

impl BspLoader {
    /// The largest extent of a face without `TEX_SPECIAL` this loader accepts (BSP-9.4,
    /// BSP-9.17, BSP-20.10).
    #[must_use]
    pub const fn max_surface_extent(self) -> i32 {
        match self {
            Self::WinQuake => BSP_MAX_SURFACE_EXTENT,
            Self::GlQuake => BSP_GL_MAX_SURFACE_EXTENT,
            Self::FitzQuake => BSP_FITZ_MAX_SURFACE_EXTENT,
        }
    }

    const fn fitz(self) -> bool {
        matches!(self, Self::FitzQuake)
    }
}

/// The lump names in id's messages and the reader's errors.
const LUMP_NAMES: [&str; BSP_HEADER_LUMPS] = [
    "entities",
    "planes",
    "textures",
    "vertexes",
    "visibility",
    "nodes",
    "texinfo",
    "faces",
    "lighting",
    "clipnodes",
    "leafs",
    "marksurfaces",
    "edges",
    "surfedges",
    "models",
];

/// The name of lump `index`, as the lump table of BSP-1 gives it.
#[must_use]
pub fn lump_name(index: usize) -> &'static str {
    LUMP_NAMES.get(index).copied().unwrap_or("unknown")
}

/// Why a BSP could not be read.
///
/// [`BspError::message`] gives id's message with the map's name where id has one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BspError {
    /// The file is shorter than its header (BSP-1.6).
    TooShort,
    /// The version is not 29 or a BSP2 variant (BSP-1.2, BSP-17.1).
    WrongVersion {
        /// The stored version.
        version: i32,
    },
    /// A lump's offset or length is negative or runs past the file (BSP-1.6).
    LumpOutOfBounds {
        /// The lump's index.
        lump: usize,
    },
    /// A lump's length is not a multiple of its element size (BSP-1.4).
    FunnyLumpSize {
        /// The lump's index.
        lump: usize,
    },
    /// An index names an element that does not exist (BSP-1.8).
    BadIndex {
        /// The lump holding the index.
        lump: usize,
        /// The element holding it.
        element: usize,
    },
    /// The textures lump's count or offsets do not fit it (BSP-4.6).
    BadTexturesLump,
    /// A texture's header or mip levels do not lie in the lump (BSP-4.5, BSP-4.6).
    BadTexture {
        /// The texture's slot.
        index: usize,
    },
    /// A texture's size is not a multiple of 16 (BSP-4.4).
    NotAligned {
        /// The texture's name.
        name: Vec<u8>,
    },
    /// A `+` texture's frame is not `0` to `9` or `A` to `J` (BSP-4.8).
    BadAnimatingTexture {
        /// The name id's message gives.
        name: Vec<u8>,
    },
    /// An animation has a gap (BSP-4.9).
    MissingFrame {
        /// The missing frame.
        frame: usize,
        /// The animation's first texture.
        name: Vec<u8>,
    },
    /// A texinfo's `miptex` names no texture slot (BSP-8.3).
    BadMiptex,
    /// A face without `TEX_SPECIAL` spans more texels than the loader accepts: 256 in
    /// WinQuake, 512 in GLQuake, 2000 in the FitzQuake lineage (BSP-9.4, BSP-9.17, BSP-20.10).
    BadSurfaceExtents,
    /// A BSP29 file has more leaves than the FitzQuake lineage accepts (BSP-20.13).
    TooManyLeafs {
        /// The number of leaves.
        count: usize,
    },
    /// A marksurface names no face (BSP-13.2).
    BadMarksurface,
    /// The BSP has no nodes (BSP-7.6).
    NoNodes,
    /// The BSP has no models (BSP-15.6).
    NoModels,
    /// A leaf's visibility runs past the lump (BSP-6.5).
    VisTruncated {
        /// The leaf.
        leaf: usize,
    },
}

impl BspError {
    /// id's message, with `name` where id names the map.
    #[must_use]
    pub fn message(&self, name: &str) -> String {
        match self {
            Self::TooShort => format!("{name} is shorter than a BSP header"),
            Self::WrongVersion { version } => format!(
                "Mod_LoadBrushModel: {name} has wrong version number ({version} should be {BSP_VERSION})"
            ),
            Self::LumpOutOfBounds { lump } => {
                format!("{name}: the {} lump runs past the file", lump_name(*lump))
            }
            Self::FunnyLumpSize { .. } => format!("MOD_LoadBmodel: funny lump size in {name}"),
            Self::BadIndex { lump, element } => format!(
                "{name}: {} {element} names an element that does not exist",
                lump_name(*lump)
            ),
            Self::BadTexturesLump => format!("{name}: the textures lump's directory runs past it"),
            Self::BadTexture { index } => {
                format!("{name}: texture {index} runs past the textures lump")
            }
            Self::NotAligned { name: texture } => {
                format!("Texture {} is not 16 aligned", texture.escape_ascii())
            }
            Self::BadAnimatingTexture { name: texture } => {
                format!("Bad animating texture {}", texture.escape_ascii())
            }
            Self::MissingFrame {
                frame,
                name: texture,
            } => {
                format!("Missing frame {frame} of {}", texture.escape_ascii())
            }
            Self::BadMiptex => "miptex >= loadmodel->numtextures".to_owned(),
            Self::BadSurfaceExtents => "Bad surface extents".to_owned(),
            Self::TooManyLeafs { count } => {
                format!("Mod_LoadLeafs: {count} leafs exceeds limit of {BSP_FITZ_MAX_LEAFS}.")
            }
            Self::BadMarksurface => "Mod_ParseMarksurfaces: bad surface number".to_owned(),
            Self::NoNodes => format!("{name} has no nodes"),
            Self::NoModels => format!("{name} has no models"),
            Self::VisTruncated { leaf } => {
                format!("{name}: the visibility of leaf {leaf} runs past the lump")
            }
        }
    }
}

impl fmt::Display for BspError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message("the map"))
    }
}

impl core::error::Error for BspError {}

/// A lump's directory entry (BSP-1.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Lump {
    /// Offset from the start of the file.
    pub fileofs: i32,
    /// Length in bytes.
    pub filelen: i32,
}

/// A plane, id's `mplane_t` (BSP-3).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    /// The normal.
    pub normal: [f32; 3],
    /// The distance from the origin.
    pub dist: f32,
    /// The stored type, not checked (BSP-3.2).
    pub plane_type: i32,
    /// Bit `j` set when `normal[j] < 0` (BSP-3.3).
    pub signbits: u8,
}

/// A texture's place in an animation (BSP-4.9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Animation {
    /// `anim_total`: the cycle's length in units.
    pub total: i32,
    /// `anim_min`: when this frame starts.
    pub min: i32,
    /// `anim_max`: when it ends.
    pub max: i32,
    /// The next frame's texture slot.
    pub next: usize,
    /// The other animation's first frame, if the texture has both.
    pub alternate: Option<usize>,
}

/// A texture, id's `texture_t` (BSP-4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Texture<'a> {
    /// The name: the bytes before the first NUL, or all 16 (BSP-4.3).
    pub name: Vec<u8>,
    /// Width in pixels, a multiple of 16.
    pub width: u32,
    /// Height in pixels, a multiple of 16.
    pub height: u32,
    /// Each mip level's offset from the start of the `miptex_t`.
    pub offsets: [u32; BSP_MIPLEVELS],
    /// The `width * height / 64 * 85` bytes after the header (BSP-4.5); for the FitzQuake
    /// lineage the `width * height` bytes of level 0 only, cut short at the end of the lump
    /// (BSP-20.5).
    pub pixels: &'a [u8],
    /// Whether the name starts with `sky`: case included for WinQuake and GLQuake (BSP-4.7),
    /// in any case for the FitzQuake lineage (BSP-20.6).
    pub sky: bool,
    /// Its animation, for a `+` texture in a cycle (BSP-4.8 to BSP-4.10).
    pub animation: Option<Animation>,
}

impl Texture<'_> {
    /// Mip level `level`'s pixels, `(width >> level) * (height >> level)` bytes at
    /// `offsets[level]`, when they lie in [`Texture::pixels`]. A texture the FitzQuake lineage
    /// read has only level 0 (BSP-20.5).
    #[must_use]
    pub fn mip(&self, level: usize) -> Option<&[u8]> {
        let offset = usize::try_from(*self.offsets.get(level)?).ok()?;
        let start = offset.checked_sub(BSP_MIPTEX_HEADER_SIZE)?;
        let size = mip_size(self.width, self.height, level)?;
        self.pixels.get(start..start.checked_add(size)?)
    }
}

/// The bytes of mip level `level` of a `width` by `height` texture.
fn mip_size(width: u32, height: u32, level: usize) -> Option<usize> {
    let shift = u32::try_from(level).ok()?;
    let width = usize::try_from(width.checked_shr(shift)?).ok()?;
    let height = usize::try_from(height.checked_shr(shift)?).ok()?;
    width.checked_mul(height)
}

/// A texinfo, id's `mtexinfo_t` (BSP-8).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Texinfo {
    /// The s and t axes and offsets.
    pub vecs: [[f32; 4]; 2],
    /// The stored `miptex`.
    pub miptex: i32,
    /// The texture slot used, or `None` for the engine's checkerboard (BSP-8.4, BSP-20.7).
    pub texture: Option<usize>,
    /// The flags: 0 when WinQuake or GLQuake use the checkerboard (BSP-8.4, BSP-8.5); the
    /// stored flags, with [`BSP_TEX_MISSING`] when the texture is missing, for the FitzQuake
    /// lineage (BSP-20.7).
    pub flags: i32,
    /// The mip bias, 1 to 4 (BSP-8.6); the FitzQuake lineage computes none (BSP-20.7).
    pub mipadjust: i32,
}

/// A face, id's `msurface_t` (BSP-9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Face {
    /// Its plane.
    pub planenum: usize,
    /// The stored side; any value but 0 is the back (BSP-9.1).
    pub side: i32,
    /// Its first surfedge.
    pub firstedge: usize,
    /// Its number of surfedges.
    pub numedges: usize,
    /// Its texinfo.
    pub texinfo: usize,
    /// Up to four light styles, ended by 255 (BSP-9.6).
    pub styles: [u8; BSP_MAXLIGHTMAPS],
    /// Its first light sample, or -1 (BSP-9.6).
    pub lightofs: i32,
    /// `texturemins`, kept in 16 bits as id's `short` field (BSP-9.5, BSP-9.10, BSP-9.14).
    pub texturemins: [i16; 2],
    /// `extents`, kept in 16 bits as id's `short` field (BSP-9.5, BSP-9.10, BSP-9.14).
    pub extents: [i16; 2],
    /// `SURF_` flags (BSP-9.1, BSP-9.5, BSP-12.6).
    pub flags: u32,
    /// Lightmap width and height in samples, from the extents before BSP-9.5 (BSP-9.7);
    /// 0 by 0 when either stored extent is negative (BSP-9.16).
    pub lightmap_size: [usize; 2],
}

impl Face {
    /// The number of light styles before the first 255 (BSP-9.6).
    #[must_use]
    pub fn style_count(&self) -> usize {
        self.styles
            .iter()
            .take_while(|&&style| style != 255)
            .count()
    }
}

/// A node, id's `mnode_t` as read (BSP-7, BSP-17).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Node {
    /// The splitting plane.
    pub planenum: usize,
    /// Front and back: a node when 0 or more, else leaf `-1 - c` (BSP-7.2), after the
    /// FitzQuake lineage's reading of the stored value (BSP-20.15, BSP-20.16).
    pub children: [i32; 2],
    /// The bounding box's minimum.
    pub mins: [f32; 3],
    /// The bounding box's maximum.
    pub maxs: [f32; 3],
    /// The first face on the node's plane.
    pub firstface: usize,
    /// The number of faces on it.
    pub numfaces: usize,
}

/// A clipnode, id's `dclipnode_t` (BSP-11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClipNode {
    /// The splitting plane.
    pub planenum: i32,
    /// Front and back: a clipnode when 0 or more, else a contents value, after the FitzQuake
    /// lineage's reading of the stored value (BSP-20.17).
    pub children: [i32; 2],
}

/// A leaf, id's `mleaf_t` as read (BSP-12, BSP-17).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Leaf {
    /// A `CONTENTS_` value, not checked (BSP-12.2).
    pub contents: i32,
    /// The offset of its visibility set, or -1 (BSP-6.1).
    pub visofs: i32,
    /// The bounding box's minimum.
    pub mins: [f32; 3],
    /// The bounding box's maximum.
    pub maxs: [f32; 3],
    /// Its first marksurface.
    pub firstmarksurface: usize,
    /// Its number of marksurfaces.
    pub nummarksurfaces: usize,
    /// Water, sky, slime and lava volumes (BSP-12.4).
    pub ambient_level: [u8; BSP_NUM_AMBIENTS],
}

/// A model, id's `dmodel_t` after `Mod_LoadSubmodels` (BSP-15).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Model {
    /// The bounds' minimum, lowered by 1 (BSP-15.4).
    pub mins: [f32; 3],
    /// The bounds' maximum, raised by 1 (BSP-15.4).
    pub maxs: [f32; 3],
    /// Read and not used (BSP-15.5).
    pub origin: [f32; 3],
    /// The first node of hull 0 and clipnode of hulls 1 to 3.
    pub headnode: [i32; BSP_MAX_MAP_HULLS],
    /// Leaves with visibility, not counting leaf 0.
    pub visleafs: i32,
    /// The first face drawn for it.
    pub firstface: usize,
    /// The number of faces.
    pub numfaces: usize,
    /// `RadiusFromBounds` (BSP-15.4).
    pub radius: f32,
}

/// A loaded BSP.
#[derive(Clone, Debug, PartialEq)]
pub struct Bsp<'a> {
    /// The format.
    pub version: BspVersion,
    /// The directory.
    pub lumps: [Lump; BSP_HEADER_LUMPS],
    /// The entities lump.
    pub entity_lump: &'a [u8],
    /// The planes.
    pub planes: Vec<Plane>,
    /// The texture slots, `None` for a missing texture (BSP-4.2); empty when the lump is.
    pub textures: Vec<Option<Texture<'a>>>,
    /// The vertexes.
    pub vertexes: Vec<[f32; 3]>,
    /// The visibility lump.
    pub visibility: &'a [u8],
    /// The nodes.
    pub nodes: Vec<Node>,
    /// The texinfo.
    pub texinfo: Vec<Texinfo>,
    /// The faces.
    pub faces: Vec<Face>,
    /// The lighting lump.
    pub lighting: &'a [u8],
    /// The clipnodes.
    pub clipnodes: Vec<ClipNode>,
    /// The leaves.
    pub leafs: Vec<Leaf>,
    /// The marksurfaces: face numbers.
    pub marksurfaces: Vec<usize>,
    /// The edges' vertex numbers.
    pub edges: Vec<[u32; 2]>,
    /// The surfedges.
    pub surfedges: Vec<i32>,
    /// The models.
    pub models: Vec<Model>,
}

fn le_bytes<const N: usize>(bytes: &[u8], offset: usize) -> [u8; N] {
    offset
        .checked_add(N)
        .and_then(|end| bytes.get(offset..end))
        .and_then(|slice| <[u8; N]>::try_from(slice).ok())
        .unwrap_or([0; N])
}

fn i32_at(bytes: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(le_bytes(bytes, offset))
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(le_bytes(bytes, offset))
}

fn i16_at(bytes: &[u8], offset: usize) -> i16 {
    i16::from_le_bytes(le_bytes(bytes, offset))
}

fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(le_bytes(bytes, offset))
}

fn f32_at(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(le_bytes(bytes, offset))
}

fn vec3_at(bytes: &[u8], offset: usize) -> [f32; 3] {
    [0, 4, 8].map(|step| f32_at(bytes, offset.saturating_add(step)))
}

/// A 16-bit bound as a float, as id's `minmaxs` holds it.
fn short3_at(bytes: &[u8], offset: usize) -> [f32; 3] {
    [0, 2, 4].map(|step| f32::from(i16_at(bytes, offset.saturating_add(step))))
}

/// An index read from a file, which must be in `0..count` (BSP-1.8).
fn index(value: i64, count: usize, lump: usize, element: usize) -> Result<usize, BspError> {
    usize::try_from(value)
        .ok()
        .filter(|&value| value < count)
        .ok_or(BspError::BadIndex { lump, element })
}

/// A range `first..first + count` read from a file, which must lie in `0..len` (BSP-1.8).
fn range(
    first: i64,
    count: i64,
    len: usize,
    lump: usize,
    element: usize,
) -> Result<(usize, usize), BspError> {
    let bad = BspError::BadIndex { lump, element };
    let first = usize::try_from(first).map_err(|_| bad.clone())?;
    let count = usize::try_from(count).map_err(|_| bad.clone())?;
    if first.checked_add(count).is_none_or(|end| end > len) {
        return Err(bad);
    }
    Ok((first, count))
}

/// A child of a node or clipnode: a node when 0 or more, else a leaf or contents
/// (BSP-7.2, BSP-11.1).
fn child_ok(child: i32, nodes: usize, leaves: Option<usize>) -> bool {
    match usize::try_from(child) {
        Ok(node) => node < nodes,
        Err(_) => leaves.is_none_or(|leaves| {
            usize::try_from(child.wrapping_neg().wrapping_sub(1)).is_ok_and(|leaf| leaf < leaves)
        }),
    }
}

/// A node child as QuakeSpasm's `Mod_LoadNodes_S`, `_L1` and `_L2` read the stored `child`
/// of a node in a lump of `nodes` nodes, given `leaves` leaves, written as BSP-7.2 writes it:
/// node `n` as `n` and leaf `l` as `-1 - l`. A leaf that does not exist is leaf 0
/// (BSP-20.15, BSP-20.16).
fn fitz_node_child(child: i32, version: BspVersion, nodes: usize, leaves: usize) -> i32 {
    let leaf = match version {
        BspVersion::Bsp29 => {
            // `p = (unsigned short)LittleShort(in->children[j])`, then `p = 65535 - p`.
            let stored = u16::from_le_bytes(low_16(child).to_le_bytes());
            if usize::from(stored) < nodes {
                return i32::from(stored);
            }
            i64::from(u16::MAX).wrapping_sub(i64::from(stored))
        }
        BspVersion::Bsp2Psb | BspVersion::Bsp2 => {
            let first = if version == BspVersion::Bsp2 { 1 } else { 0 };
            if usize::try_from(child).is_ok_and(|node| node >= first && node < nodes) {
                return child;
            }
            // `p = 0xffffffff - p`, stored back in an int: -1 - p.
            i64::from(child.wrapping_neg().wrapping_sub(1))
        }
    };
    usize::try_from(leaf)
        .ok()
        .filter(|&leaf| leaf < leaves)
        .and_then(|leaf| i32::try_from(leaf).ok())
        .map_or(-1, |leaf| leaf.wrapping_neg().wrapping_sub(1))
}

/// A BSP29 clipnode child as QuakeSpasm's `Mod_LoadClipnodes` reads the stored `child` in a
/// lump of `clipnodes` clipnodes: unsigned, less 65536 when it is not below `clipnodes`
/// (BSP-20.17).
fn fitz_clipnode_child(child: i32, clipnodes: usize) -> i32 {
    let stored = u16::from_le_bytes(low_16(child).to_le_bytes());
    if usize::from(stored) < clipnodes {
        i32::from(stored)
    } else {
        i32::from(stored).wrapping_sub(65536)
    }
}

/// A vertex's texture coordinate on one axis as WinQuake's and GLQuake's x86 builds compute
/// it: each product exact, added in the order `((x * s[0] + z * s[2]) + y * s[1]) + s[3]` at
/// 53-bit precision, then rounded once to single precision (BSP-9.11).
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec BSP-9.11: the sum is rounded to single precision when id's build stores it"
)]
fn x87_texture_coordinate(vertex: [f32; 3], vecs: [f32; 4]) -> f32 {
    let [x, y, z] = vertex.map(f64::from);
    let [s0, s1, s2, offset] = vecs.map(f64::from);
    // A product of two singles has at most 48 significant bits, so a double holds it exactly.
    let sum = x * s0 + z * s2 + y * s1 + offset;
    sum as f32
}

/// Whether the original x86 build replaces `bound` with a coordinate when it compares `lower`
/// with `upper`: unless `lower >= upper`, so an unordered (NaN) comparison replaces it
/// (BSP-9.12).
#[expect(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "spec BSP-9.12: id's build treats an unordered comparison as less"
)]
fn x87_replaces(lower: f32, upper: f32) -> bool {
    !(lower >= upper)
}

/// A vertex's texture coordinate on one axis as QuakeSpasm computes it: every product and sum
/// in double precision, added in C's order `((x * s[0] + y * s[1]) + z * s[2]) + s[3]`, then
/// rounded once to single precision (BSP-20.9).
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec BSP-20.9: QuakeSpasm stores the double sum in a float"
)]
fn double_texture_coordinate(vertex: [f32; 3], vecs: [f32; 4]) -> f32 {
    let [x, y, z] = vertex.map(f64::from);
    let [s0, s1, s2, offset] = vecs.map(f64::from);
    let sum = x * s0 + y * s1 + z * s2 + offset;
    sum as f32
}

/// A `double` converted to `int` as the original x86 build does it: in range it truncates,
/// otherwise it gives the integer indefinite value, `i32::MIN` (BSP-9.13).
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec BSP-9.13: in range the value is already whole, so nothing is lost"
)]
fn x86_double_to_int(value: f64) -> i32 {
    let value = value.trunc();
    if value >= f64::from(i32::MIN) && value <= f64::from(i32::MAX) {
        value as i32
    } else {
        i32::MIN
    }
}

/// The low 16 bits of `value` as a signed value, as C's conversion to `short` keeps them on
/// id's x86 builds (BSP-9.14).
fn low_16(value: i32) -> i16 {
    let [low, high, ..] = value.to_le_bytes();
    i16::from_le_bytes([low, high])
}

/// The lumps of a header and the file they index.
struct Reader<'a> {
    bytes: &'a [u8],
    lumps: [Lump; BSP_HEADER_LUMPS],
    version: BspVersion,
}

impl<'a> Reader<'a> {
    fn lump(&self, index: usize) -> &'a [u8] {
        let lump = self.lumps.get(index).copied().unwrap_or_default();
        usize::try_from(lump.fileofs)
            .ok()
            .zip(usize::try_from(lump.filelen).ok())
            .and_then(|(start, len)| self.bytes.get(start..start.checked_add(len)?))
            .unwrap_or_default()
    }

    /// The lump's elements of `size` bytes, failing on a length that is not a multiple
    /// (BSP-1.4).
    fn elements(
        &self,
        index: usize,
        size: usize,
    ) -> Result<core::slice::ChunksExact<'a, u8>, BspError> {
        let lump = self.lump(index);
        if size == 0 || lump.len().checked_rem(size) != Some(0) {
            return Err(BspError::FunnyLumpSize { lump: index });
        }
        Ok(lump.chunks_exact(size))
    }

    const fn bsp2(&self) -> bool {
        !matches!(self.version, BspVersion::Bsp29)
    }
}

impl<'a> Bsp<'a> {
    /// Loads a BSP held in memory, id's `Mod_LoadBrushModel`.
    ///
    /// Implements BSP-1 to BSP-17 and BSP-20: checks the version, then loads the lumps in id's
    /// order (BSP-1.5), each with id's checks and the bounds and index checks the spec adds.
    /// `loader` picks WinQuake's, GLQuake's or the FitzQuake lineage's rules where they differ:
    /// the limit on face extents (BSP-9.4, BSP-9.17, BSP-20.10), whether faces in leaves that
    /// are not empty get [`SURF_UNDERWATER`] (BSP-12.6), and the rest of BSP-20.
    ///
    /// # Errors
    ///
    /// The first [`BspError`] in load order.
    pub fn read(bytes: &'a [u8], loader: BspLoader) -> Result<Self, BspError> {
        let header = bytes.get(..BSP_HEADER_SIZE).ok_or(BspError::TooShort)?;
        let version = match i32_at(header, 0) {
            BSP_VERSION => BspVersion::Bsp29,
            BSP_2PSB_VERSION => BspVersion::Bsp2Psb,
            BSP_BSP2_VERSION => BspVersion::Bsp2,
            version => return Err(BspError::WrongVersion { version }),
        };
        let mut lumps = [Lump::default(); BSP_HEADER_LUMPS];
        for (index, lump) in lumps.iter_mut().enumerate() {
            let at = index.saturating_mul(8).saturating_add(4);
            *lump = Lump {
                fileofs: i32_at(header, at),
                filelen: i32_at(header, at.saturating_add(4)),
            };
            let fits = usize::try_from(lump.fileofs)
                .ok()
                .zip(usize::try_from(lump.filelen).ok())
                .and_then(|(start, len)| start.checked_add(len))
                .is_some_and(|end| end <= bytes.len());
            if !fits {
                return Err(BspError::LumpOutOfBounds { lump: index });
            }
        }
        let reader = Reader {
            bytes,
            lumps,
            version,
        };
        let mut bsp = Self {
            version,
            lumps,
            entity_lump: &[],
            planes: Vec::new(),
            textures: Vec::new(),
            vertexes: Vec::new(),
            visibility: &[],
            nodes: Vec::new(),
            texinfo: Vec::new(),
            faces: Vec::new(),
            lighting: &[],
            clipnodes: Vec::new(),
            leafs: Vec::new(),
            marksurfaces: Vec::new(),
            edges: Vec::new(),
            surfedges: Vec::new(),
            models: Vec::new(),
        };
        bsp.mod_load_vertexes(&reader)?;
        bsp.mod_load_edges(&reader)?;
        bsp.mod_load_surfedges(&reader)?;
        bsp.mod_load_textures(&reader, loader)?;
        bsp.lighting = reader.lump(BSP_LUMP_LIGHTING);
        bsp.mod_load_planes(&reader)?;
        bsp.mod_load_texinfo(&reader, loader)?;
        bsp.mod_load_faces(&reader, loader)?;
        bsp.mod_load_marksurfaces(&reader)?;
        bsp.visibility = reader.lump(BSP_LUMP_VISIBILITY);
        bsp.mod_load_leafs(&reader, loader)?;
        bsp.mod_load_nodes(&reader, loader)?;
        bsp.mod_load_clipnodes(&reader, loader)?;
        bsp.entity_lump = reader.lump(BSP_LUMP_ENTITIES);
        bsp.mod_load_submodels(&reader)?;
        Ok(bsp)
    }

    /// id's `Mod_LoadVertexes` (BSP-5.1).
    fn mod_load_vertexes(&mut self, reader: &Reader<'a>) -> Result<(), BspError> {
        self.vertexes = reader
            .elements(BSP_LUMP_VERTEXES, BSP_VERTEX_SIZE)?
            .map(|chunk| vec3_at(chunk, 0))
            .collect();
        Ok(())
    }

    /// id's `Mod_LoadEdges` (BSP-14.1, BSP-17.3).
    fn mod_load_edges(&mut self, reader: &Reader<'a>) -> Result<(), BspError> {
        self.edges = if reader.bsp2() {
            reader
                .elements(BSP_LUMP_EDGES, BSP_BSP2_EDGE_SIZE)?
                .map(|chunk| [u32_at(chunk, 0), u32_at(chunk, 4)])
                .collect()
        } else {
            reader
                .elements(BSP_LUMP_EDGES, BSP_EDGE_SIZE)?
                .map(|chunk| [u32::from(u16_at(chunk, 0)), u32::from(u16_at(chunk, 2))])
                .collect()
        };
        Ok(())
    }

    /// id's `Mod_LoadSurfedges` (BSP-14.2).
    fn mod_load_surfedges(&mut self, reader: &Reader<'a>) -> Result<(), BspError> {
        self.surfedges = reader
            .elements(BSP_LUMP_SURFEDGES, BSP_SURFEDGE_SIZE)?
            .map(|chunk| i32_at(chunk, 0))
            .collect();
        Ok(())
    }

    /// id's `Mod_LoadTextures`: the slots, then the animations (BSP-4, BSP-20.3 to BSP-20.6).
    fn mod_load_textures(
        &mut self,
        reader: &Reader<'a>,
        loader: BspLoader,
    ) -> Result<(), BspError> {
        let lump = reader.lump(BSP_LUMP_TEXTURES);
        if lump.is_empty() {
            return Ok(());
        }
        let count = usize::try_from(i32_at(lump, 0))
            .ok()
            .filter(|_| lump.len() >= 4)
            .ok_or(BspError::BadTexturesLump)?;
        let table_end = count
            .checked_mul(4)
            .and_then(|size| size.checked_add(4))
            .filter(|&end| end <= lump.len())
            .ok_or(BspError::BadTexturesLump)?;
        let table = lump.get(4..table_end).unwrap_or_default();
        let mut textures = Vec::with_capacity(count);
        for (index, slot) in table.as_chunks::<4>().0.iter().enumerate() {
            let offset = i32::from_le_bytes(*slot);
            if offset == -1 {
                textures.push(None);
                continue;
            }
            textures.push(load_texture(lump, offset, index, loader)?);
        }
        sequence_animations(&mut textures)?;
        self.textures = textures;
        Ok(())
    }

    /// id's `Mod_LoadPlanes` (BSP-3).
    fn mod_load_planes(&mut self, reader: &Reader<'a>) -> Result<(), BspError> {
        self.planes = reader
            .elements(BSP_LUMP_PLANES, BSP_PLANE_SIZE)?
            .map(|chunk| {
                let normal = vec3_at(chunk, 0);
                let mut signbits = 0_u8;
                for (bit, value) in normal.iter().enumerate() {
                    if *value < 0.0 {
                        signbits |= 1_u8
                            .checked_shl(u32::try_from(bit).unwrap_or(0))
                            .unwrap_or(0);
                    }
                }
                Plane {
                    normal,
                    dist: f32_at(chunk, 12),
                    plane_type: i32_at(chunk, 16),
                    signbits,
                }
            })
            .collect();
        Ok(())
    }

    /// id's `Mod_LoadTexinfo` (BSP-8, BSP-20.7).
    fn mod_load_texinfo(&mut self, reader: &Reader<'a>, loader: BspLoader) -> Result<(), BspError> {
        let mut texinfo = Vec::new();
        for chunk in reader.elements(BSP_LUMP_TEXINFO, BSP_TEXINFO_SIZE)? {
            let mut vecs = [[0.0_f32; 4]; 2];
            for (row, axis) in vecs.iter_mut().enumerate() {
                for (column, value) in axis.iter_mut().enumerate() {
                    *value = f32_at(
                        chunk,
                        row.saturating_mul(16)
                            .saturating_add(column.saturating_mul(4)),
                    );
                }
            }
            let [s, t] = vecs;
            let mean = (length(s) + length(t)) / 2.0;
            let mipadjust = if f64::from(mean) < 0.32 {
                4
            } else if f64::from(mean) < 0.49 {
                3
            } else if f64::from(mean) < 0.99 {
                2
            } else {
                1
            };
            let miptex = i32_at(chunk, 32);
            let mut flags = i32_at(chunk, 36);
            let mut texture = None;
            if loader.fitz() {
                // BSP-20.7: slot `nummiptex` is QuakeSpasm's checkerboard for lit faces, and
                // a slot past it, or an empty one, is missing.
                let slots = self.textures.len();
                let slot = usize::try_from(miptex).map_err(|_| BspError::BadMiptex)?;
                match self.textures.get(slot) {
                    Some(Some(_)) => texture = Some(slot),
                    Some(None) => flags |= BSP_TEX_MISSING,
                    None if slot == slots => {}
                    None => flags |= BSP_TEX_MISSING,
                }
            } else if reader.lump(BSP_LUMP_TEXTURES).is_empty() {
                flags = 0;
            } else {
                let slot = usize::try_from(miptex)
                    .ok()
                    .filter(|&slot| slot < self.textures.len())
                    .ok_or(BspError::BadMiptex)?;
                if self.textures.get(slot).is_some_and(Option::is_some) {
                    texture = Some(slot);
                } else {
                    flags = 0;
                }
            }
            texinfo.push(Texinfo {
                vecs,
                miptex,
                texture,
                flags,
                mipadjust,
            });
        }
        self.texinfo = texinfo;
        Ok(())
    }

    /// id's `Mod_LoadFaces` with `CalcSurfaceExtents` (BSP-9, BSP-10, BSP-14).
    fn mod_load_faces(&mut self, reader: &Reader<'a>, loader: BspLoader) -> Result<(), BspError> {
        let size = if reader.bsp2() {
            BSP_BSP2_FACE_SIZE
        } else {
            BSP_FACE_SIZE
        };
        let mut faces = Vec::new();
        for (element, chunk) in reader.elements(BSP_LUMP_FACES, size)?.enumerate() {
            let (planenum, side, firstedge, numedges, texinfo, styles_at) = if reader.bsp2() {
                (
                    i64::from(i32_at(chunk, 0)),
                    i32_at(chunk, 4),
                    i64::from(i32_at(chunk, 8)),
                    i64::from(i32_at(chunk, 12)),
                    i64::from(i32_at(chunk, 16)),
                    20,
                )
            } else {
                (
                    i64::from(i16_at(chunk, 0)),
                    i32::from(i16_at(chunk, 2)),
                    i64::from(i32_at(chunk, 4)),
                    i64::from(i16_at(chunk, 8)),
                    i64::from(i16_at(chunk, 10)),
                    12,
                )
            };
            let planenum = index(planenum, self.planes.len(), BSP_LUMP_FACES, element)?;
            let texinfo = index(texinfo, self.texinfo.len(), BSP_LUMP_FACES, element)?;
            let (firstedge, numedges) = range(
                firstedge,
                numedges,
                self.surfedges.len(),
                BSP_LUMP_FACES,
                element,
            )?;
            let styles = le_bytes(chunk, styles_at);
            let lightofs = i32_at(chunk, styles_at.saturating_add(4));
            let mut face = Face {
                planenum,
                side,
                firstedge,
                numedges,
                texinfo,
                styles,
                lightofs,
                texturemins: [0; 2],
                extents: [0; 2],
                flags: if side == 0 { 0 } else { SURF_PLANEBACK },
                lightmap_size: [0; 2],
            };
            self.calc_surface_extents(&mut face, element, loader)?;
            self.check_light(&face, element)?;
            let info = self.texinfo.get(texinfo);
            let name = info
                .and_then(|info| info.texture)
                .and_then(|slot| self.textures.get(slot))
                .and_then(Option::as_ref)
                .map(|texture| texture.name.as_slice())
                .unwrap_or_default();
            if loader.fitz() {
                face.flags |= fitz_face_flags(name, info.map_or(0, |info| info.flags), lightofs);
            } else if name.starts_with(b"sky") {
                face.flags |= SURF_DRAWSKY | SURF_DRAWTILED;
            } else if name.starts_with(b"*") {
                face.flags |= SURF_DRAWTURB | SURF_DRAWTILED;
                face.extents = [16384; 2];
                face.texturemins = [-8192; 2];
            }
            faces.push(face);
        }
        self.faces = faces;
        Ok(())
    }

    /// The face's vertex for surfedge `edge`: an edge's `v[0]`, or a negated edge's `v[1]`
    /// (BSP-14.2, BSP-14.3).
    fn surfedge_vertex(&self, edge: usize, element: usize) -> Result<[f32; 3], BspError> {
        let bad = BspError::BadIndex {
            lump: BSP_LUMP_FACES,
            element,
        };
        let surfedge = *self.surfedges.get(edge).ok_or(bad.clone())?;
        let (edge, end) = if surfedge >= 0 {
            (surfedge, 0)
        } else {
            (surfedge.checked_neg().ok_or(bad.clone())?, 1)
        };
        let edge = self
            .edges
            .get(usize::try_from(edge).map_err(|_| bad.clone())?)
            .ok_or(bad.clone())?;
        let vertex = edge.get(end).copied().ok_or(bad.clone())?;
        self.vertexes
            .get(usize::try_from(vertex).map_err(|_| bad.clone())?)
            .copied()
            .ok_or(bad)
    }

    /// The vertices of face `face`, in order (BSP-9.2, BSP-14.2).
    #[must_use]
    pub fn face_vertices(&self, face: &Face) -> Vec<[f32; 3]> {
        (face.firstedge..face.firstedge.saturating_add(face.numedges))
            .filter_map(|edge| self.surfedge_vertex(edge, 0).ok())
            .collect()
    }

    /// id's `CalcSurfaceExtents` as WinQuake's and GLQuake's x86 builds run it (BSP-9.4,
    /// BSP-9.7, BSP-9.10 to BSP-9.14, BSP-9.17), or QuakeSpasm's as its x86-64 build runs it
    /// (BSP-20.8 to BSP-20.10).
    fn calc_surface_extents(
        &self,
        face: &mut Face,
        element: usize,
        loader: BspLoader,
    ) -> Result<(), BspError> {
        let (mut mins, mut maxs) = if loader.fitz() {
            ([f32::MAX; 2], [f32::MIN; 2])
        } else {
            ([999_999.0_f32; 2], [-99_999.0_f32; 2])
        };
        let texinfo = self
            .texinfo
            .get(face.texinfo)
            .copied()
            .ok_or(BspError::BadIndex {
                lump: BSP_LUMP_FACES,
                element,
            })?;
        for edge in face.firstedge..face.firstedge.saturating_add(face.numedges) {
            let vertex = self.surfedge_vertex(edge, element)?;
            for (axis, vecs) in texinfo.vecs.iter().enumerate() {
                let (Some(min), Some(max)) = (mins.get_mut(axis), maxs.get_mut(axis)) else {
                    continue;
                };
                if loader.fitz() {
                    // BSP-20.8: C's comparisons, so a NaN replaces neither bound.
                    let value = double_texture_coordinate(vertex, *vecs);
                    if value < *min {
                        *min = value;
                    }
                    if value > *max {
                        *max = value;
                    }
                } else {
                    let value = x87_texture_coordinate(vertex, *vecs);
                    if x87_replaces(value, *min) {
                        *min = value;
                    }
                    if x87_replaces(*max, value) {
                        *max = value;
                    }
                }
            }
        }
        for axis in 0..2 {
            let min = mins.get(axis).copied().unwrap_or_default();
            let max = maxs.get(axis).copied().unwrap_or_default();
            let bmin = x86_double_to_int(f64::from(min / 16.0).floor());
            let bmax = x86_double_to_int(f64::from(max / 16.0).ceil());
            let texturemins = low_16(bmin.wrapping_mul(16));
            let extents = low_16(bmax.wrapping_sub(bmin).wrapping_mul(16));
            if let Some(slot) = face.texturemins.get_mut(axis) {
                *slot = texturemins;
            }
            if let Some(slot) = face.extents.get_mut(axis) {
                *slot = extents;
            }
            if texinfo.flags & BSP_TEX_SPECIAL == 0
                && i32::from(extents) > loader.max_surface_extent()
            {
                return Err(BspError::BadSurfaceExtents);
            }
        }
        face.lightmap_size = lightmap_size(face.extents);
        Ok(())
    }

    /// Checks that a face's light samples lie in the lighting lump (BSP-10.2).
    fn check_light(&self, face: &Face, element: usize) -> Result<(), BspError> {
        if face.lightofs == -1 {
            return Ok(());
        }
        let bad = BspError::BadIndex {
            lump: BSP_LUMP_FACES,
            element,
        };
        let start = usize::try_from(face.lightofs).map_err(|_| bad.clone())?;
        let [width, height] = face.lightmap_size;
        let size = width
            .checked_mul(height)
            .and_then(|size| size.checked_mul(face.style_count()))
            .ok_or(bad.clone())?;
        if start
            .checked_add(size)
            .is_none_or(|end| end > self.lighting.len())
        {
            return Err(bad);
        }
        Ok(())
    }

    /// The light samples of `face`: `smax * tmax` for each style, or `None` when it has
    /// none (BSP-9.6, BSP-9.7, BSP-10.1); empty for a face with a negative stored extent
    /// (BSP-9.16).
    #[must_use]
    pub fn face_samples(&self, face: &Face) -> Option<&'a [u8]> {
        let start = usize::try_from(face.lightofs).ok()?;
        let [width, height] = face.lightmap_size;
        let size = width.checked_mul(height)?.checked_mul(face.style_count())?;
        self.lighting.get(start..start.checked_add(size)?)
    }

    /// id's `Mod_LoadMarksurfaces` (BSP-13, BSP-17.3).
    fn mod_load_marksurfaces(&mut self, reader: &Reader<'a>) -> Result<(), BspError> {
        let values: Vec<u32> = if reader.bsp2() {
            reader
                .elements(BSP_LUMP_MARKSURFACES, BSP_BSP2_MARKSURFACE_SIZE)?
                .map(|chunk| u32_at(chunk, 0))
                .collect()
        } else {
            reader
                .elements(BSP_LUMP_MARKSURFACES, 2)?
                .map(|chunk| u32::from(u16_at(chunk, 0)))
                .collect()
        };
        let mut marksurfaces = Vec::with_capacity(values.len());
        for value in values {
            let face = usize::try_from(value)
                .ok()
                .filter(|&face| face < self.faces.len())
                .ok_or(BspError::BadMarksurface)?;
            marksurfaces.push(face);
        }
        self.marksurfaces = marksurfaces;
        Ok(())
    }

    /// id's `Mod_LoadLeafs` (BSP-12, BSP-17.4), GLQuake's flag on the faces of a leaf that is
    /// not empty (BSP-12.6), and the FitzQuake lineage's limit on leaves (BSP-20.13).
    fn mod_load_leafs(&mut self, reader: &Reader<'a>, loader: BspLoader) -> Result<(), BspError> {
        let size = match reader.version {
            BspVersion::Bsp29 => BSP_LEAF_SIZE,
            BspVersion::Bsp2Psb => BSP_2PSB_LEAF_SIZE,
            BspVersion::Bsp2 => BSP_BSP2_LEAF_SIZE,
        };
        let chunks = reader.elements(BSP_LUMP_LEAFS, size)?;
        let count = chunks.len();
        if loader.fitz() && reader.version == BspVersion::Bsp29 && count > BSP_FITZ_MAX_LEAFS {
            return Err(BspError::TooManyLeafs { count });
        }
        let mut leafs = Vec::with_capacity(count);
        for (element, chunk) in chunks.enumerate() {
            let (mins, maxs, marks_at) = match reader.version {
                BspVersion::Bsp29 | BspVersion::Bsp2Psb => {
                    (short3_at(chunk, 8), short3_at(chunk, 14), 20)
                }
                BspVersion::Bsp2 => (vec3_at(chunk, 8), vec3_at(chunk, 20), 32),
            };
            let (first, count, ambient_at) = if reader.bsp2() {
                (
                    i64::from(u32_at(chunk, marks_at)),
                    i64::from(u32_at(chunk, marks_at.saturating_add(4))),
                    marks_at.saturating_add(8),
                )
            } else {
                (
                    i64::from(u16_at(chunk, marks_at)),
                    i64::from(u16_at(chunk, marks_at.saturating_add(2))),
                    marks_at.saturating_add(4),
                )
            };
            let (firstmarksurface, nummarksurfaces) = range(
                first,
                count,
                self.marksurfaces.len(),
                BSP_LUMP_LEAFS,
                element,
            )?;
            let visofs = i32_at(chunk, 4);
            // BSP-6.6: leaf 0's offset is never read, and 0 into an empty lump is no set.
            let unread = element == 0 || (visofs == 0 && self.visibility.is_empty());
            if visofs != -1 && !unread {
                index(
                    i64::from(visofs),
                    self.visibility.len(),
                    BSP_LUMP_LEAFS,
                    element,
                )?;
            }
            let contents = i32_at(chunk, 0);
            if loader == BspLoader::GlQuake && contents != BSP_CONTENTS_EMPTY {
                let marks = self
                    .marksurfaces
                    .iter()
                    .skip(firstmarksurface)
                    .take(nummarksurfaces);
                for &face in marks {
                    if let Some(face) = self.faces.get_mut(face) {
                        face.flags |= SURF_UNDERWATER;
                    }
                }
            }
            leafs.push(Leaf {
                contents,
                visofs,
                mins,
                maxs,
                firstmarksurface,
                nummarksurfaces,
                ambient_level: le_bytes(chunk, ambient_at),
            });
        }
        self.leafs = leafs;
        Ok(())
    }

    /// id's `Mod_LoadNodes` (BSP-7, BSP-17.4), with the FitzQuake lineage's reading of the
    /// children (BSP-20.15, BSP-20.16).
    fn mod_load_nodes(&mut self, reader: &Reader<'a>, loader: BspLoader) -> Result<(), BspError> {
        let size = match reader.version {
            BspVersion::Bsp29 => BSP_NODE_SIZE,
            BspVersion::Bsp2Psb => BSP_2PSB_NODE_SIZE,
            BspVersion::Bsp2 => BSP_BSP2_NODE_SIZE,
        };
        let chunks = reader.elements(BSP_LUMP_NODES, size)?;
        let count = chunks.len();
        let mut nodes = Vec::with_capacity(count);
        for (element, chunk) in chunks.enumerate() {
            let (children, mins, maxs, faces_at) = match reader.version {
                BspVersion::Bsp29 => (
                    [i32::from(i16_at(chunk, 4)), i32::from(i16_at(chunk, 6))],
                    short3_at(chunk, 8),
                    short3_at(chunk, 14),
                    20,
                ),
                BspVersion::Bsp2Psb => (
                    [i32_at(chunk, 4), i32_at(chunk, 8)],
                    short3_at(chunk, 12),
                    short3_at(chunk, 18),
                    24,
                ),
                BspVersion::Bsp2 => (
                    [i32_at(chunk, 4), i32_at(chunk, 8)],
                    vec3_at(chunk, 12),
                    vec3_at(chunk, 24),
                    36,
                ),
            };
            let (first, numfaces) = if reader.bsp2() {
                (
                    i64::from(u32_at(chunk, faces_at)),
                    i64::from(u32_at(chunk, faces_at.saturating_add(4))),
                )
            } else {
                (
                    i64::from(u16_at(chunk, faces_at)),
                    i64::from(u16_at(chunk, faces_at.saturating_add(2))),
                )
            };
            let planenum = index(
                i64::from(i32_at(chunk, 0)),
                self.planes.len(),
                BSP_LUMP_NODES,
                element,
            )?;
            let (firstface, numfaces) =
                range(first, numfaces, self.faces.len(), BSP_LUMP_NODES, element)?;
            let children = if loader.fitz() {
                children
                    .map(|child| fitz_node_child(child, reader.version, count, self.leafs.len()))
            } else {
                children
            };
            if !children
                .iter()
                .all(|&child| child_ok(child, count, Some(self.leafs.len())))
            {
                return Err(BspError::BadIndex {
                    lump: BSP_LUMP_NODES,
                    element,
                });
            }
            nodes.push(Node {
                planenum,
                children,
                mins,
                maxs,
                firstface,
                numfaces,
            });
        }
        if nodes.is_empty() {
            return Err(BspError::NoNodes);
        }
        self.nodes = nodes;
        Ok(())
    }

    /// id's `Mod_LoadClipnodes` (BSP-11, BSP-17.3), with the FitzQuake lineage's reading of a
    /// BSP29 clipnode's children (BSP-20.17).
    fn mod_load_clipnodes(
        &mut self,
        reader: &Reader<'a>,
        loader: BspLoader,
    ) -> Result<(), BspError> {
        let size = if reader.bsp2() {
            BSP_BSP2_CLIPNODE_SIZE
        } else {
            BSP_CLIPNODE_SIZE
        };
        let chunks = reader.elements(BSP_LUMP_CLIPNODES, size)?;
        let count = chunks.len();
        let mut clipnodes = Vec::with_capacity(count);
        for (element, chunk) in chunks.enumerate() {
            let children = if reader.bsp2() {
                [i32_at(chunk, 4), i32_at(chunk, 8)]
            } else if loader.fitz() {
                [4, 6].map(|at| fitz_clipnode_child(i32::from(i16_at(chunk, at)), count))
            } else {
                [i32::from(i16_at(chunk, 4)), i32::from(i16_at(chunk, 6))]
            };
            let planenum = i32_at(chunk, 0);
            index(
                i64::from(planenum),
                self.planes.len(),
                BSP_LUMP_CLIPNODES,
                element,
            )?;
            if !children.iter().all(|&child| child_ok(child, count, None)) {
                return Err(BspError::BadIndex {
                    lump: BSP_LUMP_CLIPNODES,
                    element,
                });
            }
            clipnodes.push(ClipNode { planenum, children });
        }
        self.clipnodes = clipnodes;
        Ok(())
    }

    /// id's `Mod_LoadSubmodels` with `RadiusFromBounds` (BSP-15).
    fn mod_load_submodels(&mut self, reader: &Reader<'a>) -> Result<(), BspError> {
        let mut models = Vec::new();
        for (element, chunk) in reader
            .elements(BSP_LUMP_MODELS, BSP_MODEL_SIZE)?
            .enumerate()
        {
            let mins = vec3_at(chunk, 0).map(|value| value - 1.0);
            let maxs = vec3_at(chunk, 12).map(|value| value + 1.0);
            let headnode = [36, 40, 44, 48].map(|at| i32_at(chunk, at));
            let (firstface, numfaces) = range(
                i64::from(i32_at(chunk, 56)),
                i64::from(i32_at(chunk, 60)),
                self.faces.len(),
                BSP_LUMP_MODELS,
                element,
            )?;
            // Hull 3 is not used (BSP-11.3), so only hulls 0 to 2 must name a node.
            let [hull0, hull1, hull2, _] = headnode;
            let heads_ok = child_ok(hull0, self.nodes.len(), None)
                && child_ok(hull1, self.clipnodes.len(), None)
                && child_ok(hull2, self.clipnodes.len(), None);
            if !heads_ok {
                return Err(BspError::BadIndex {
                    lump: BSP_LUMP_MODELS,
                    element,
                });
            }
            let mut corner = [0.0_f32; 3];
            for ((value, min), max) in corner.iter_mut().zip(mins).zip(maxs) {
                *value = if min.abs() > max.abs() {
                    min.abs()
                } else {
                    max.abs()
                };
            }
            models.push(Model {
                mins,
                maxs,
                origin: vec3_at(chunk, 24),
                headnode,
                visleafs: i32_at(chunk, 52),
                firstface,
                numfaces,
                radius: length(corner),
            });
        }
        if models.is_empty() {
            return Err(BspError::NoModels);
        }
        self.models = models;
        Ok(())
    }

    /// The entity text: the lump up to its first NUL, or all of it (BSP-2.2).
    #[must_use]
    pub fn entities(&self) -> &'a [u8] {
        let end = self
            .entity_lump
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(self.entity_lump.len());
        self.entity_lump.get(..end).unwrap_or_default()
    }

    /// The bytes of a visibility row: `(visleafs + 7) >> 3` for model 0's `visleafs`
    /// (BSP-6.2).
    #[must_use]
    pub fn vis_row_size(&self) -> usize {
        let visleafs = self.models.first().map_or(0, |model| model.visleafs);
        usize::try_from(visleafs.saturating_add(7) >> 3).unwrap_or(0)
    }

    /// The leaves `leaf` sees, id's `Mod_LeafPVS` and `Mod_DecompressVis`: bit `k` is leaf
    /// `k + 1` (BSP-6.2 to BSP-6.5).
    ///
    /// # Errors
    ///
    /// [`BspError::VisTruncated`] when its set runs past the lump, and
    /// [`BspError::BadIndex`] for a leaf that does not exist.
    pub fn leaf_pvs(&self, leaf: usize) -> Result<Vec<u8>, BspError> {
        let row = self.vis_row_size();
        let found = self.leafs.get(leaf).ok_or(BspError::BadIndex {
            lump: BSP_LUMP_LEAFS,
            element: leaf,
        })?;
        let Ok(start) = usize::try_from(found.visofs) else {
            return Ok(vec![0xFF; row]);
        };
        if leaf == 0 || self.visibility.is_empty() {
            return Ok(vec![0xFF; row]);
        }
        let truncated = BspError::VisTruncated { leaf };
        let mut input = self
            .visibility
            .get(start..)
            .ok_or(truncated.clone())?
            .iter();
        let mut out = Vec::new();
        // A do-while in id's code: at least one byte or run is read.
        loop {
            let byte = *input.next().ok_or(truncated.clone())?;
            if byte == 0 {
                let count = *input.next().ok_or(truncated.clone())?;
                out.extend(core::iter::repeat_n(0, usize::from(count)));
            } else {
                out.push(byte);
            }
            if out.len() >= row {
                break;
            }
        }
        out.truncate(row);
        Ok(out)
    }

    /// Hull 0 as clipnodes, id's `Mod_MakeHull0`: each node with its plane, a child node
    /// kept and a child leaf replaced by its contents (BSP-7.5).
    #[must_use]
    pub fn hull0(&self) -> Vec<ClipNode> {
        self.nodes
            .iter()
            .map(|node| ClipNode {
                planenum: i32::try_from(node.planenum).unwrap_or(i32::MAX),
                children: node.children.map(|child| {
                    if child >= 0 {
                        child
                    } else {
                        usize::try_from(child.wrapping_neg().wrapping_sub(1))
                            .ok()
                            .and_then(|leaf| self.leafs.get(leaf))
                            .map_or(BSP_CONTENTS_SOLID, |leaf| leaf.contents)
                    }
                }),
            })
            .collect()
    }
}

/// The name of brush model `n` above 0, `*<n>` (BSP-15.3).
#[must_use]
pub fn model_name(n: usize) -> String {
    format!("*{n}")
}

/// The server's precache index of brush model `n`: the world is 1 and `*<n>` is `n + 1`
/// (BSP-15.3).
#[must_use]
pub const fn model_precache_index(n: usize) -> usize {
    n.saturating_add(1)
}

/// The hull a `SOLID_BSP` model is collided with for another entity of x size `size`,
/// id's `SV_HullForEntity`: 0 under 3, 1 up to 32, otherwise 2 (BSP-11.3).
#[must_use]
pub fn hull_for_size(size: f32) -> usize {
    if size < 3.0 {
        0
    } else if size <= 32.0 {
        1
    } else {
        2
    }
}

/// id's `Length`: the square root of the float sum of squares, through `double`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec BSP-8.6: id's Length stores the double square root in a float"
)]
fn length<const N: usize>(vector: [f32; N]) -> f32 {
    let mut sum = 0.0_f32;
    for value in vector.iter().take(3) {
        sum += value * value;
    }
    f64::from(sum).sqrt() as f32
}

/// Loads the texture at `offset` in the textures lump (BSP-4.3 to BSP-4.5), or leaves its slot
/// empty where the FitzQuake lineage does (BSP-20.3).
fn load_texture(
    lump: &[u8],
    offset: i32,
    index: usize,
    loader: BspLoader,
) -> Result<Option<Texture<'_>>, BspError> {
    let bad = BspError::BadTexture { index };
    let start = usize::try_from(offset).map_err(|_| bad.clone())?;
    let header = start
        .checked_add(BSP_MIPTEX_HEADER_SIZE)
        .and_then(|end| lump.get(start..end))
        .ok_or(bad.clone())?;
    let raw_name = header.get(..16).unwrap_or_default();
    let name_len = raw_name.iter().position(|&byte| byte == 0).unwrap_or(16);
    let name = raw_name.get(..name_len).unwrap_or_default().to_vec();
    let width = u32_at(header, 16);
    let height = u32_at(header, 20);
    let offsets = [24, 28, 32, 36].map(|at| u32_at(header, at));
    if loader.fitz() {
        return fitz_texture(lump, start, name, width, height, offsets, index);
    }
    if width & 15 != 0 || height & 15 != 0 {
        return Err(BspError::NotAligned { name });
    }
    let pixels = width
        .checked_mul(height)
        .and_then(|area| area.checked_div(64))
        .and_then(|area| area.checked_mul(85))
        .and_then(|size| usize::try_from(size).ok())
        .ok_or(bad.clone())?;
    let data_start = start.saturating_add(BSP_MIPTEX_HEADER_SIZE);
    let data = data_start
        .checked_add(pixels)
        .and_then(|end| lump.get(data_start..end))
        .ok_or(bad.clone())?;
    let texture = Texture {
        sky: name.starts_with(b"sky"),
        name,
        width,
        height,
        offsets,
        pixels: data,
        animation: None,
    };
    for level in 0..BSP_MIPLEVELS {
        if texture.mip(level).is_none() {
            return Err(bad);
        }
    }
    Ok(Some(texture))
}

/// QuakeSpasm's `Mod_LoadTextures` for one texture whose header lies at `start`: a texture
/// with no pixels leaves its slot empty, any size loads, and only level 0's
/// `width * height` bytes are kept, cut short at the end of the lump (BSP-20.3 to BSP-20.6).
fn fitz_texture(
    lump: &[u8],
    start: usize,
    name: Vec<u8>,
    width: u32,
    height: u32,
    offsets: [u32; BSP_MIPLEVELS],
    index: usize,
) -> Result<Option<Texture<'_>>, BspError> {
    if width == 0 || height == 0 {
        return Ok(None);
    }
    // BSP-20.5: `int pixels = mt->width*mt->height` multiplies as unsigned; a product that
    // does not fit an int would make QuakeSpasm copy a negative count.
    let pixels = usize::try_from(width.wrapping_mul(height))
        .ok()
        .filter(|&pixels| i32::try_from(pixels).is_ok())
        .ok_or(BspError::BadTexture { index })?;
    let data_start = start.saturating_add(BSP_MIPTEX_HEADER_SIZE);
    let available = lump.len().saturating_sub(data_start);
    let data = lump
        .get(data_start..data_start.saturating_add(pixels.min(available)))
        .unwrap_or_default();
    Ok(Some(Texture {
        sky: fitz_sky(&name),
        name,
        width,
        height,
        offsets,
        pixels: data,
        animation: None,
    }))
}

/// The frame a `+` texture's second character names: `(false, n)` for frame `n` of the
/// primary animation, `(true, n)` of the alternate one (BSP-4.8).
fn frame_of(name: &[u8]) -> Option<(bool, usize)> {
    let character = name.get(1).copied().unwrap_or(0).to_ascii_uppercase();
    match character {
        b'0'..=b'9' => Some((false, usize::from(character.wrapping_sub(b'0')))),
        b'A'..=b'J' => Some((true, usize::from(character.wrapping_sub(b'A')))),
        _ => None,
    }
}

/// id's animation sequencing in `Mod_LoadTextures` (BSP-4.8 to BSP-4.10).
fn sequence_animations(textures: &mut [Option<Texture<'_>>]) -> Result<(), BspError> {
    for first in 0..textures.len() {
        let Some(texture) = textures.get(first).and_then(Option::as_ref) else {
            continue;
        };
        if texture.name.first() != Some(&b'+') || texture.animation.is_some() {
            continue;
        }
        let name = texture.name.clone();
        let suffix = name.get(2..).unwrap_or_default().to_vec();
        let mut anims: [Option<usize>; 10] = [None; 10];
        let mut altanims: [Option<usize>; 10] = [None; 10];
        let mut max = 0_usize;
        let mut altmax = 0_usize;
        let bad = || BspError::BadAnimatingTexture { name: name.clone() };
        match frame_of(&name).ok_or_else(bad)? {
            (false, frame) => {
                if let Some(slot) = anims.get_mut(frame) {
                    *slot = Some(first);
                }
                max = frame.saturating_add(1);
            }
            (true, frame) => {
                if let Some(slot) = altanims.get_mut(frame) {
                    *slot = Some(first);
                }
                altmax = frame.saturating_add(1);
            }
        }
        for other in first.saturating_add(1)..textures.len() {
            let Some(candidate) = textures.get(other).and_then(Option::as_ref) else {
                continue;
            };
            if candidate.name.first() != Some(&b'+')
                || candidate.name.get(2..).unwrap_or_default() != suffix.as_slice()
            {
                continue;
            }
            // id names the first texture of the animation, not the bad one.
            let (alternate, frame) = frame_of(&candidate.name).ok_or_else(bad)?;
            let (slots, top) = if alternate {
                (&mut altanims, &mut altmax)
            } else {
                (&mut anims, &mut max)
            };
            if let Some(slot) = slots.get_mut(frame) {
                *slot = Some(other);
            }
            *top = (*top).max(frame.saturating_add(1));
        }
        link(
            textures,
            &anims,
            max,
            altanims.first().copied().flatten().filter(|_| altmax > 0),
            &name,
        )?;
        link(
            textures,
            &altanims,
            altmax,
            anims.first().copied().flatten().filter(|_| max > 0),
            &name,
        )?;
    }
    Ok(())
}

/// Links one animation's frames in a cycle (BSP-4.9).
fn link(
    textures: &mut [Option<Texture<'_>>],
    frames: &[Option<usize>; 10],
    count: usize,
    alternate: Option<usize>,
    name: &[u8],
) -> Result<(), BspError> {
    let total = i32::try_from(count)
        .unwrap_or(0)
        .saturating_mul(BSP_ANIM_CYCLE);
    // id links frame by frame and stops at the first gap, so check them all first.
    if let Some(frame) = (0..count).find(|&frame| frames.get(frame).copied().flatten().is_none()) {
        return Err(BspError::MissingFrame {
            frame,
            name: name.to_vec(),
        });
    }
    for frame in 0..count {
        let slot = frames.get(frame).copied().flatten().unwrap_or_default();
        let next = frame
            .saturating_add(1)
            .checked_rem(count)
            .and_then(|next| frames.get(next).copied().flatten())
            .unwrap_or(slot);
        let start = i32::try_from(frame)
            .unwrap_or(0)
            .saturating_mul(BSP_ANIM_CYCLE);
        if let Some(texture) = textures.get_mut(slot).and_then(Option::as_mut) {
            texture.animation = Some(Animation {
                total,
                min: start,
                max: start.saturating_add(BSP_ANIM_CYCLE),
                next,
                alternate,
            });
        }
    }
    Ok(())
}

/// Whether `name` starts with `sky` in any case, QuakeSpasm's `q_strncasecmp(name, "sky", 3)`
/// (BSP-20.6).
fn fitz_sky(name: &[u8]) -> bool {
    name.get(..3)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"sky"))
}

/// The flags QuakeSpasm's `Mod_LoadFaces` adds to a face from its texture's name, its
/// texinfo's flags and its `lightofs` (BSP-20.6, BSP-20.11).
fn fitz_face_flags(name: &[u8], texinfo_flags: i32, lightofs: i32) -> u32 {
    if fitz_sky(name) {
        return SURF_DRAWSKY | SURF_DRAWTILED;
    }
    match name.first() {
        Some(b'*' | b'!') => {
            let mut flags = SURF_DRAWTURB;
            if texinfo_flags & BSP_TEX_SPECIAL != 0 {
                flags |= SURF_DRAWTILED;
            }
            let liquid = name.get(1..).unwrap_or_default();
            flags |= if liquid.starts_with(b"lava") {
                SURF_DRAWLAVA
            } else if liquid.starts_with(b"slime") {
                SURF_DRAWSLIME
            } else if liquid.starts_with(b"tele") {
                SURF_DRAWTELE
            } else {
                SURF_DRAWWATER
            };
            flags
        }
        Some(b'{') => SURF_DRAWFENCE,
        _ if texinfo_flags & BSP_TEX_MISSING != 0 => {
            if lightofs == -1 {
                SURF_NOTEXTURE | SURF_DRAWTILED
            } else {
                SURF_NOTEXTURE
            }
        }
        _ => 0,
    }
}

/// A face's lightmap size in samples: `(extent >> 4) + 1` on each axis (BSP-9.7), or 0 by 0
/// when either stored extent is negative, which no id renderer draws light from (BSP-9.16).
fn lightmap_size(extents: [i16; 2]) -> [usize; 2] {
    if extents.iter().any(|&extent| extent < 0) {
        return [0, 0];
    }
    extents.map(|extent| usize::from(extent.unsigned_abs() >> 4).saturating_add(1))
}

/// A BSP with its entity lump replaced by `text` and a NUL, as `qbsp` writes it (BSP-2.2):
/// the header, then every lump in the order of its offset in the original file, each at an
/// offset that is a multiple of 4, the others' bytes unchanged. For editing the entities of
/// a map with no `.map` source (BSP-19).
///
/// # Errors
///
/// Any [`BspError`] the original file gives [`Bsp::read`] with `loader`, or
/// [`BspError::LumpOutOfBounds`] when the new file would pass 2 GiB.
pub fn replace_entities(bytes: &[u8], text: &[u8], loader: BspLoader) -> Result<Vec<u8>, BspError> {
    let bsp = Bsp::read(bytes, loader)?;
    let mut entities = text.to_vec();
    entities.push(0);
    let mut order: Vec<usize> = (0..BSP_HEADER_LUMPS).collect();
    order.sort_by_key(|&index| bsp.lumps.get(index).map_or(0, |lump| lump.fileofs));
    let mut out = bytes.get(..BSP_HEADER_SIZE).unwrap_or_default().to_vec();
    let mut directory = [Lump::default(); BSP_HEADER_LUMPS];
    let reader = Reader {
        bytes,
        lumps: bsp.lumps,
        version: bsp.version,
    };
    for index in order {
        while out.len().checked_rem(4) != Some(0) {
            out.push(0);
        }
        let data = if index == BSP_LUMP_ENTITIES {
            entities.as_slice()
        } else {
            reader.lump(index)
        };
        let too_big = BspError::LumpOutOfBounds { lump: index };
        let entry = Lump {
            fileofs: i32::try_from(out.len()).map_err(|_| too_big.clone())?,
            filelen: i32::try_from(data.len()).map_err(|_| too_big.clone())?,
        };
        if let Some(slot) = directory.get_mut(index) {
            *slot = entry;
        }
        out.extend_from_slice(data);
        i32::try_from(out.len()).map_err(|_| too_big)?;
    }
    for (index, lump) in directory.iter().enumerate() {
        let at = index.saturating_mul(8).saturating_add(4);
        for (offset, value) in [(at, lump.fileofs), (at.saturating_add(4), lump.filelen)] {
            if let Some(slot) = offset
                .checked_add(4)
                .and_then(|end| out.get_mut(offset..end))
            {
                slot.copy_from_slice(&value.to_le_bytes());
            }
        }
    }
    Ok(out)
}
