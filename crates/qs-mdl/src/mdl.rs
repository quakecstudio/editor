// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/model.c, Mod_LoadAliasModel, Mod_LoadAliasSkin, Mod_LoadAliasSkinGroup, Mod_LoadAliasFrame, Mod_LoadAliasGroup

//! Alias models: id's `.mdl` files, read as `Mod_LoadAliasModel` reads them (MDL-1 to MDL-5).

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tests;

use core::fmt;

use crate::cursor::{self, le_i32};

/// The four bytes an alias model starts with, id's `IDPOLYHEADER` (MDL-1.2).
pub const MDL_MAGIC: [u8; 4] = *b"IDPO";
/// The only version the engine loads, id's `ALIAS_VERSION` (MDL-1.3).
pub const MDL_VERSION: i32 = 6;
/// Bytes in the header, id's `sizeof(mdl_t)` (MDL-1.1).
pub const MDL_HEADER_SIZE: usize = 84;
/// The tallest skin, id's `MAX_LBM_HEIGHT` (MDL-1.4).
pub const MDL_MAX_SKIN_HEIGHT: i32 = 480;
/// The most vertices, the software renderer's `MAXALIASVERTS` (MDL-1.4).
pub const MDL_MAX_VERTS: i32 = 2000;
/// What `size` is multiplied by, id's `ALIAS_BASE_SIZE_RATIO` (MDL-1.6).
pub const MDL_SIZE_RATIO: f64 = 1.0 / 11.0;
/// GLQuake's most vertices, `MAXALIASVERTS` in `gl_model.h` (MDL-1.8).
pub const MDL_GL_MAX_VERTS: i32 = 1024;
/// GLQuake's most skins, `MAX_SKINS` (MDL-1.8).
pub const MDL_GL_MAX_SKINS: i32 = 32;
/// GLQuake's triangle array, `MAXALIASTRIS` (MDL-1.8).
pub const MDL_GL_MAX_TRIS: i32 = 2048;
/// GLQuake's pose array, `MAXALIASFRAMES` (MDL-1.8).
pub const MDL_GL_MAX_POSES: usize = 256;
/// Sync type `ST_SYNC`.
pub const MDL_ST_SYNC: i32 = 0;
/// Sync type `ST_RAND`.
pub const MDL_ST_RAND: i32 = 1;
/// `EF_ROCKET`: a rocket trail (MDL-1.9).
pub const MDL_EF_ROCKET: i32 = 1;
/// `EF_GRENADE`: a grenade trail.
pub const MDL_EF_GRENADE: i32 = 2;
/// `EF_GIB`: a blood trail.
pub const MDL_EF_GIB: i32 = 4;
/// `EF_ROTATE`: the model spins.
pub const MDL_EF_ROTATE: i32 = 8;
/// `EF_TRACER`: a green split trail.
pub const MDL_EF_TRACER: i32 = 16;
/// `EF_ZOMGIB`: a small blood trail.
pub const MDL_EF_ZOMGIB: i32 = 32;
/// `EF_TRACER2`: an orange split trail.
pub const MDL_EF_TRACER2: i32 = 64;
/// `EF_TRACER3`: a purple trail.
pub const MDL_EF_TRACER3: i32 = 128;
/// Skin type `ALIAS_SKIN_SINGLE` (MDL-2.1).
pub const MDL_SKIN_SINGLE: i32 = 0;
/// Skin type `ALIAS_SKIN_GROUP`; any non-zero type is read as a group (MDL-2.1).
pub const MDL_SKIN_GROUP: i32 = 1;
/// Bytes in a texture coordinate, `sizeof(stvert_t)` (MDL-3.1).
pub const MDL_STVERT_SIZE: usize = 12;
/// The seam flag, `ALIAS_ONSEAM` (MDL-3.2).
pub const MDL_ALIAS_ONSEAM: i32 = 0x20;
/// Bytes in a triangle, `sizeof(dtriangle_t)` (MDL-4.1).
pub const MDL_TRIANGLE_SIZE: usize = 16;
/// `DT_FACES_FRONT` (MDL-4.2).
pub const MDL_DT_FACES_FRONT: i32 = 0x10;
/// Frame type `ALIAS_SINGLE` (MDL-5.1).
pub const MDL_FRAME_SINGLE: i32 = 0;
/// Frame type `ALIAS_GROUP`; any non-zero type is read as a group (MDL-5.1).
pub const MDL_FRAME_GROUP: i32 = 1;
/// Bytes in a frame's header, `sizeof(daliasframe_t)` (MDL-5.2).
pub const MDL_FRAME_HEADER_SIZE: usize = 24;
/// Bytes in a frame group's header, `sizeof(daliasgroup_t)` (MDL-5.4).
pub const MDL_GROUP_HEADER_SIZE: usize = 12;
/// Entries in the renderer's normal table, `NUMVERTEXNORMALS` (MDL-5.3).
pub const MDL_NUMVERTEXNORMALS: u8 = 162;

/// Why an alias model could not be read.
///
/// [`MdlError::message`] gives id's message with the model's name where id has one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MdlError {
    /// The file does not start with `IDPO` (MDL-1.2).
    WrongId,
    /// The version is not 6 (MDL-1.3).
    WrongVersion {
        /// The stored version.
        version: i32,
    },
    /// The skin is taller than 480 (MDL-1.4).
    SkinTooTall,
    /// `numverts` is 0 or less (MDL-1.4).
    NoVertices,
    /// `numverts` is over 2000 (MDL-1.4).
    TooManyVertices,
    /// `numtris` is 0 or less (MDL-1.4).
    NoTriangles,
    /// `skinwidth` is not a multiple of 4 (MDL-1.4).
    SkinWidth,
    /// `numskins` is under 1 (MDL-1.4).
    InvalidSkinCount {
        /// The stored `numskins`.
        numskins: i32,
    },
    /// The skin's size is negative or overflows (MDL-1.5).
    BadSkinSize {
        /// The stored width.
        width: i32,
        /// The stored height.
        height: i32,
    },
    /// A skin group has fewer than one skin (MDL-2.4).
    InvalidSkinGroupCount {
        /// The stored count.
        count: i32,
    },
    /// A skin group's interval is 0 or less (MDL-2.3).
    SkinInterval,
    /// A triangle's vertex is not below `numverts` (MDL-4.3).
    BadVertexIndex {
        /// The triangle.
        triangle: usize,
    },
    /// `numframes` is under 1 (MDL-1.4).
    InvalidFrameCount {
        /// The stored `numframes`.
        numframes: i32,
    },
    /// A frame group has fewer than one frame (MDL-5.6).
    InvalidFrameGroupCount {
        /// The stored count.
        count: i32,
    },
    /// A frame group's interval is 0 or less (MDL-5.5).
    FrameInterval,
    /// A structure runs past the end of the file (MDL-1.5, MDL-2.5, MDL-5.6).
    Truncated {
        /// Offset of the structure that does not fit.
        offset: usize,
    },
}

impl MdlError {
    /// id's message, with `name` where id names the model.
    #[must_use]
    pub fn message(&self, name: &str) -> String {
        match self {
            Self::WrongId => format!("{name} is not an alias model (no IDPO id)"),
            Self::WrongVersion { version } => {
                format!("{name} has wrong version number ({version} should be {MDL_VERSION})")
            }
            Self::SkinTooTall => {
                format!("model {name} has a skin taller than {MDL_MAX_SKIN_HEIGHT}")
            }
            Self::NoVertices => format!("model {name} has no vertices"),
            Self::TooManyVertices => format!("model {name} has too many vertices"),
            Self::NoTriangles => format!("model {name} has no triangles"),
            Self::SkinWidth => "Mod_LoadAliasModel: skinwidth not multiple of 4".to_owned(),
            Self::InvalidSkinCount { numskins } => {
                format!("Mod_LoadAliasModel: Invalid # of skins: {numskins}")
            }
            Self::BadSkinSize { width, height } => {
                format!("model {name} has a skin of {width} by {height}, which is invalid")
            }
            Self::InvalidSkinGroupCount { count } => {
                format!("model {name} has a skin group of {count} skins")
            }
            Self::SkinInterval => "Mod_LoadAliasSkinGroup: interval<=0".to_owned(),
            Self::BadVertexIndex { triangle } => {
                format!("model {name}: triangle {triangle} names a vertex that does not exist")
            }
            Self::InvalidFrameCount { numframes } => {
                format!("Mod_LoadAliasModel: Invalid # of frames: {numframes}")
            }
            Self::InvalidFrameGroupCount { count } => {
                format!("model {name} has a frame group of {count} frames")
            }
            Self::FrameInterval => "Mod_LoadAliasGroup: interval<=0".to_owned(),
            Self::Truncated { offset } => {
                format!("model {name}: data at offset {offset} runs past the end of the file")
            }
        }
    }
}

impl fmt::Display for MdlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message("the model"))
    }
}

impl core::error::Error for MdlError {}

/// An alias model's header, id's `mdl_t` (MDL-1.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MdlHeader {
    /// The size of one vertex step on each axis.
    pub scale: [f32; 3],
    /// The position of vertex (0, 0, 0).
    pub scale_origin: [f32; 3],
    /// Not used by the engine.
    pub bounding_radius: f32,
    /// Not used by the engine.
    pub eye_position: [f32; 3],
    /// Skin entries, at least 1.
    pub numskins: i32,
    /// Skin width in pixels, a multiple of 4.
    pub skinwidth: i32,
    /// Skin height in pixels, at most 480.
    pub skinheight: i32,
    /// Vertices per frame, 1 to 2000.
    pub numverts: i32,
    /// Triangles, at least 1.
    pub numtris: i32,
    /// Frame entries, at least 1.
    pub numframes: i32,
    /// `ST_SYNC` or `ST_RAND`; not checked.
    pub synctype: i32,
    /// `EF_` bits; not checked (MDL-1.9).
    pub flags: i32,
    /// The stored average triangle size.
    pub size: f32,
}

/// One skin entry (MDL-2.1).
#[derive(Clone, Debug, PartialEq)]
pub enum MdlSkin<'a> {
    /// Type 0: one skin's `skinwidth * skinheight` pixels (MDL-2.2).
    Single(&'a [u8]),
    /// Any other type: a group (MDL-2.3).
    Group {
        /// The stored type.
        skin_type: i32,
        /// One interval per skin, each greater than 0 or NaN.
        intervals: Vec<f32>,
        /// The skins.
        skins: Vec<&'a [u8]>,
    },
}

/// A vertex's position on the skin, id's `stvert_t` (MDL-3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StVert {
    /// Non-zero for a vertex on the seam (MDL-3.2).
    pub onseam: i32,
    /// Pixels from the left (MDL-3.3).
    pub s: i32,
    /// Pixels from the top.
    pub t: i32,
}

/// A triangle, id's `dtriangle_t` (MDL-4.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Triangle {
    /// 0 for a triangle on the back half of the skin (MDL-4.2).
    pub facesfront: i32,
    /// Its three vertices, each below `numverts` (MDL-4.3).
    pub vertindex: [usize; 3],
}

/// A packed vertex, id's `trivertx_t` (MDL-5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TriVertX {
    /// x, y and z steps.
    pub v: [u8; 3],
    /// The entry of the normal table (MDL-5.3).
    pub lightnormalindex: u8,
}

impl TriVertX {
    fn read(bytes: &[u8], offset: usize) -> Self {
        let byte = |at: usize| bytes.get(offset.saturating_add(at)).copied().unwrap_or(0);
        Self {
            v: [byte(0), byte(1), byte(2)],
            lightnormalindex: byte(3),
        }
    }
}

/// A frame: its bounding box, name and vertices (MDL-5.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MdlFrame<'a> {
    /// The bounding box's minimum.
    pub bboxmin: TriVertX,
    /// The bounding box's maximum.
    pub bboxmax: TriVertX,
    /// The bytes before the first NUL of its 16-byte name, or all 16.
    pub name: Vec<u8>,
    verts: &'a [u8],
}

impl MdlFrame<'_> {
    /// Vertex `index`.
    #[must_use]
    pub fn vertex(&self, index: usize) -> Option<TriVertX> {
        let offset = index.checked_mul(4)?;
        self.verts
            .get(offset..offset.checked_add(4)?)
            .map(|bytes| TriVertX::read(bytes, 0))
    }

    /// The number of vertices.
    #[must_use]
    pub const fn vertex_count(&self) -> usize {
        match self.verts.len().checked_div(4) {
            Some(count) => count,
            None => 0,
        }
    }
}

/// One frame entry (MDL-5.1).
#[derive(Clone, Debug, PartialEq)]
pub enum MdlFrameEntry<'a> {
    /// Type 0: one frame.
    Single(MdlFrame<'a>),
    /// Any other type: a group (MDL-5.4).
    Group {
        /// The stored type.
        frame_type: i32,
        /// The group's bounding box's minimum.
        bboxmin: TriVertX,
        /// The group's bounding box's maximum.
        bboxmax: TriVertX,
        /// One interval per frame, each greater than 0 or NaN.
        intervals: Vec<f32>,
        /// The frames, at least one.
        frames: Vec<MdlFrame<'a>>,
    },
}

impl MdlFrameEntry<'_> {
    /// The entry's name: its frame's, or for a group its last frame's (MDL-5.4).
    #[must_use]
    pub fn name(&self) -> &[u8] {
        match self {
            Self::Single(frame) => &frame.name,
            Self::Group { frames, .. } => frames.last().map_or(&[], |frame| &frame.name),
        }
    }

    /// The entry's bounding box: its frame's, or the group's (MDL-5.4).
    #[must_use]
    pub const fn bbox(&self) -> (TriVertX, TriVertX) {
        match self {
            Self::Single(frame) => (frame.bboxmin, frame.bboxmax),
            Self::Group {
                bboxmin, bboxmax, ..
            } => (*bboxmin, *bboxmax),
        }
    }
}

/// A limit of GLQuake's loader that a model exceeds (MDL-1.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlLimit {
    /// More than 1024 vertices.
    Vertices,
    /// More than 32 skins.
    Skins,
    /// More than 2048 triangles.
    Triangles,
    /// More than 256 poses, counting each frame of a group.
    Poses,
}

/// A loaded alias model, whose skins and vertices borrow the file's bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct Mdl<'a> {
    /// The header.
    pub header: MdlHeader,
    /// `numskins` skin entries.
    pub skins: Vec<MdlSkin<'a>>,
    /// `numverts` texture coordinates.
    pub stverts: Vec<StVert>,
    /// `numtris` triangles.
    pub triangles: Vec<Triangle>,
    /// `numframes` frame entries.
    pub frames: Vec<MdlFrameEntry<'a>>,
}

/// A cursor over an alias model.
type Cursor<'a> = cursor::Cursor<'a, MdlError>;

/// A count of 1 or more read from the file, or `error`.
fn count(value: i32, error: MdlError) -> Result<usize, MdlError> {
    usize::try_from(value)
        .ok()
        .filter(|&count| count >= 1)
        .ok_or(error)
}

impl<'a> Mdl<'a> {
    /// Loads an alias model held in memory, id's `Mod_LoadAliasModel`.
    ///
    /// Implements MDL-1 to MDL-5: the header's checks in id's order, then the skins,
    /// texture coordinates, triangles and frames, checking every size against the file.
    ///
    /// # Errors
    ///
    /// The first [`MdlError`] in id's order.
    pub fn read(bytes: &'a [u8]) -> Result<Self, MdlError> {
        // One 32-bit comparison, which coverage-guided fuzzers can solve.
        if le_i32(bytes, 0) != Some(i32::from_le_bytes(MDL_MAGIC)) {
            return Err(MdlError::WrongId);
        }
        let header = bytes
            .get(..MDL_HEADER_SIZE)
            .ok_or(MdlError::Truncated { offset: 0 })?;
        let word = |index: usize| le_i32(header, index.saturating_mul(4)).unwrap_or_default();
        let float = |index: usize| f32::from_bits(word(index).cast_unsigned());
        let version = word(1);
        if version != MDL_VERSION {
            return Err(MdlError::WrongVersion { version });
        }
        let header = MdlHeader {
            scale: [float(2), float(3), float(4)],
            scale_origin: [float(5), float(6), float(7)],
            bounding_radius: float(8),
            eye_position: [float(9), float(10), float(11)],
            numskins: word(12),
            skinwidth: word(13),
            skinheight: word(14),
            numverts: word(15),
            numtris: word(16),
            numframes: word(17),
            synctype: word(18),
            flags: word(19),
            size: float(20),
        };
        if header.skinheight > MDL_MAX_SKIN_HEIGHT {
            return Err(MdlError::SkinTooTall);
        }
        if header.numverts <= 0 {
            return Err(MdlError::NoVertices);
        }
        if header.numverts > MDL_MAX_VERTS {
            return Err(MdlError::TooManyVertices);
        }
        if header.numtris <= 0 {
            return Err(MdlError::NoTriangles);
        }
        if header.skinwidth & 3 != 0 {
            return Err(MdlError::SkinWidth);
        }
        let numskins = count(
            header.numskins,
            MdlError::InvalidSkinCount {
                numskins: header.numskins,
            },
        )?;
        let bad_size = MdlError::BadSkinSize {
            width: header.skinwidth,
            height: header.skinheight,
        };
        if header.skinwidth < 0 || header.skinheight < 0 {
            return Err(bad_size);
        }
        let skinsize = header
            .skinwidth
            .checked_mul(header.skinheight)
            .and_then(|size| usize::try_from(size).ok())
            .ok_or(bad_size)?;
        let mut cursor = Cursor::new(bytes, MDL_HEADER_SIZE, |offset| MdlError::Truncated {
            offset,
        });
        let mut skins = Vec::new();
        for _ in 0..numskins {
            let skin_type = cursor.int()?;
            skins.push(if skin_type == MDL_SKIN_SINGLE {
                MdlSkin::Single(cursor.take(skinsize)?)
            } else {
                mod_load_alias_skin_group(&mut cursor, skin_type, skinsize)?
            });
        }
        let numverts = usize::try_from(header.numverts).unwrap_or(0);
        let mut stverts = Vec::with_capacity(numverts);
        for _ in 0..numverts {
            stverts.push(StVert {
                onseam: cursor.int()?,
                s: cursor.int()?,
                t: cursor.int()?,
            });
        }
        let numtris = usize::try_from(header.numtris).unwrap_or(0);
        let mut triangles = Vec::new();
        for triangle in 0..numtris {
            let facesfront = cursor.int()?;
            let mut vertindex = [0_usize; 3];
            for vertex in &mut vertindex {
                *vertex = usize::try_from(cursor.int()?)
                    .ok()
                    .filter(|&vertex| vertex < numverts)
                    .ok_or(MdlError::BadVertexIndex { triangle })?;
            }
            triangles.push(Triangle {
                facesfront,
                vertindex,
            });
        }
        let numframes = count(
            header.numframes,
            MdlError::InvalidFrameCount {
                numframes: header.numframes,
            },
        )?;
        let mut frames = Vec::new();
        for _ in 0..numframes {
            let frame_type = cursor.int()?;
            frames.push(if frame_type == MDL_FRAME_SINGLE {
                MdlFrameEntry::Single(mod_load_alias_frame(&mut cursor, numverts)?)
            } else {
                mod_load_alias_group(&mut cursor, frame_type, numverts)?
            });
        }
        Ok(Self {
            header,
            skins,
            stverts,
            triangles,
            frames,
        })
    }

    /// The size the loader keeps: `size * (1.0 / 11.0)`, the product in double stored in a
    /// float (MDL-1.6).
    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "spec MDL-1.6: id stores the double product in a float"
    )]
    pub fn size(&self) -> f32 {
        (f64::from(self.header.size) * MDL_SIZE_RATIO) as f32
    }

    /// Every alias model's bounds, `mins` then `maxs` (MDL-1.7).
    #[must_use]
    pub const fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        ([-16.0; 3], [16.0; 3])
    }

    /// A vertex's position in the model: `v * scale + scale_origin` on each axis, in single
    /// precision (MDL-5.3).
    #[must_use]
    pub fn position(&self, vertex: TriVertX) -> [f32; 3] {
        let mut out = [0.0; 3];
        for (((value, step), scale), origin) in out
            .iter_mut()
            .zip(vertex.v)
            .zip(self.header.scale)
            .zip(self.header.scale_origin)
        {
            *value = f32::from(step) * scale + origin;
        }
        out
    }

    /// The `s` a triangle uses for one of its vertices: moved right by `skinwidth / 2` for a
    /// seam vertex of a triangle that does not face front (MDL-3.2).
    #[must_use]
    pub fn skin_s(&self, triangle: &Triangle, corner: usize) -> Option<i32> {
        let vertex = self.stverts.get(*triangle.vertindex.get(corner)?)?;
        let half = self.header.skinwidth.checked_div(2).unwrap_or(0);
        if vertex.onseam != 0 && triangle.facesfront == 0 {
            Some(vertex.s.wrapping_add(half))
        } else {
            Some(vertex.s)
        }
    }

    /// Every frame vertex whose normal index is past the 162-entry table, as (entry, frame
    /// in the entry, vertex) (MDL-5.3).
    #[must_use]
    pub fn bad_normals(&self) -> Vec<(usize, usize, usize)> {
        let mut out = Vec::new();
        for (entry, frames) in self.frames.iter().enumerate() {
            let frames: Vec<&MdlFrame<'_>> = match frames {
                MdlFrameEntry::Single(frame) => vec![frame],
                MdlFrameEntry::Group { frames, .. } => frames.iter().collect(),
            };
            for (index, frame) in frames.into_iter().enumerate() {
                for vertex in 0..frame.vertex_count() {
                    if frame
                        .vertex(vertex)
                        .is_some_and(|found| found.lightnormalindex >= MDL_NUMVERTEXNORMALS)
                    {
                        out.push((entry, index, vertex));
                    }
                }
            }
        }
        out
    }

    /// The limits of GLQuake's loader the model exceeds (MDL-1.8).
    #[must_use]
    pub fn gl_limits(&self) -> Vec<GlLimit> {
        let poses: usize = self
            .frames
            .iter()
            .map(|entry| match entry {
                MdlFrameEntry::Single(_) => 1,
                MdlFrameEntry::Group { frames, .. } => frames.len(),
            })
            .sum();
        let mut out = Vec::new();
        if self.header.numverts > MDL_GL_MAX_VERTS {
            out.push(GlLimit::Vertices);
        }
        if self.header.numskins > MDL_GL_MAX_SKINS {
            out.push(GlLimit::Skins);
        }
        if self.header.numtris > MDL_GL_MAX_TRIS {
            out.push(GlLimit::Triangles);
        }
        if poses > MDL_GL_MAX_POSES {
            out.push(GlLimit::Poses);
        }
        out
    }
}

/// id's `Mod_LoadAliasSkinGroup` (MDL-2.3 to MDL-2.5).
fn mod_load_alias_skin_group<'a>(
    cursor: &mut Cursor<'a>,
    skin_type: i32,
    skinsize: usize,
) -> Result<MdlSkin<'a>, MdlError> {
    let stored = cursor.int()?;
    let numskins = count(stored, MdlError::InvalidSkinGroupCount { count: stored })?;
    let intervals = cursor.intervals(numskins, |_| MdlError::SkinInterval)?;
    let mut skins = Vec::new();
    for _ in 0..numskins {
        skins.push(cursor.take(skinsize)?);
    }
    Ok(MdlSkin::Group {
        skin_type,
        intervals,
        skins,
    })
}

/// id's `Mod_LoadAliasFrame` (MDL-5.2).
fn mod_load_alias_frame<'a>(
    cursor: &mut Cursor<'a>,
    numverts: usize,
) -> Result<MdlFrame<'a>, MdlError> {
    let header = cursor.take(MDL_FRAME_HEADER_SIZE)?;
    let raw_name = header.get(8..24).unwrap_or_default();
    let name_len = raw_name.iter().position(|&byte| byte == 0).unwrap_or(16);
    let verts = cursor.take(numverts.saturating_mul(4))?;
    Ok(MdlFrame {
        bboxmin: TriVertX::read(header, 0),
        bboxmax: TriVertX::read(header, 4),
        name: raw_name.get(..name_len).unwrap_or_default().to_vec(),
        verts,
    })
}

/// id's `Mod_LoadAliasGroup` (MDL-5.4 to MDL-5.6).
fn mod_load_alias_group<'a>(
    cursor: &mut Cursor<'a>,
    frame_type: i32,
    numverts: usize,
) -> Result<MdlFrameEntry<'a>, MdlError> {
    let header = cursor.take(MDL_GROUP_HEADER_SIZE)?;
    let stored = le_i32(header, 0).unwrap_or_default();
    let numframes = count(stored, MdlError::InvalidFrameGroupCount { count: stored })?;
    let intervals = cursor.intervals(numframes, |_| MdlError::FrameInterval)?;
    let mut frames = Vec::new();
    for _ in 0..numframes {
        frames.push(mod_load_alias_frame(cursor, numverts)?);
    }
    Ok(MdlFrameEntry::Group {
        frame_type,
        bboxmin: TriVertX::read(header, 4),
        bboxmax: TriVertX::read(header, 8),
        intervals,
        frames,
    })
}
