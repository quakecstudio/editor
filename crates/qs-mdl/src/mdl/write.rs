// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73: qutils/MODELGEN/MODELGEN.C, WriteModelFile, WriteFrame, WriteGroupBBox

//! An alias model held whole, to change and to write back (MDL-6): the file's parts as
//! [`super::Mdl`] reads them, owned, and [`Model::to_bytes`], which lays them out as id's
//! `WriteModelFile` does. A model read and written with nothing changed is the file it was
//! read from, byte for byte.

use core::fmt;

use super::{
    MDL_FRAME_NAME_SIZE, MDL_FRAME_SINGLE, MDL_MAGIC, MDL_SKIN_SINGLE, MDL_VERSION, Mdl, MdlError, MdlFrame, MdlFrameEntry, MdlHeader, MdlSkin,
    StVert, Triangle, TriVertX,
};

/// One skin entry of a model held whole (MDL-2.1).
#[derive(Clone, Debug, PartialEq)]
pub enum ModelSkin {
    /// One skin's pixels, `skinwidth * skinheight` palette indices.
    Single(Vec<u8>),
    /// A group of skins shown in turn.
    Group {
        /// The stored type, [`super::MDL_SKIN_GROUP`] in a model that is made.
        skin_type: i32,
        /// One interval per skin.
        intervals: Vec<f32>,
        /// The skins.
        skins: Vec<Vec<u8>>,
    },
}

/// A frame of a model held whole (MDL-5.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelFrame {
    /// The bounding box's minimum.
    pub bboxmin: TriVertX,
    /// The bounding box's maximum.
    pub bboxmax: TriVertX,
    /// The 16 bytes of its name, the name ending at its first NUL.
    pub name: [u8; MDL_FRAME_NAME_SIZE],
    /// Its vertices, one per vertex of the model.
    pub verts: Vec<TriVertX>,
}

impl ModelFrame {
    /// The frame's name: the bytes before the first NUL.
    #[must_use]
    pub fn name(&self) -> &[u8] {
        let len = self
            .name
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(MDL_FRAME_NAME_SIZE);
        self.name.get(..len).unwrap_or_default()
    }

    /// Gives the frame the name `name`, the rest of its 16 bytes NUL.
    ///
    /// # Errors
    ///
    /// [`MdlWriteError::FrameName`] for a name of more than 15 bytes, or one holding a NUL:
    /// the engine reads the name as a C string in 16 bytes.
    pub fn set_name(&mut self, name: &[u8]) -> Result<(), MdlWriteError> {
        if name.len() >= MDL_FRAME_NAME_SIZE || name.contains(&0) {
            return Err(MdlWriteError::FrameName);
        }
        let mut bytes = [0; MDL_FRAME_NAME_SIZE];
        for (to, from) in bytes.iter_mut().zip(name) {
            *to = *from;
        }
        self.name = bytes;
        Ok(())
    }

    /// The frame `frame` as read.
    fn of(frame: &MdlFrame<'_>) -> Self {
        Self {
            bboxmin: frame.bboxmin,
            bboxmax: frame.bboxmax,
            name: frame.raw_name,
            verts: (0..frame.vertex_count())
                .filter_map(|index| frame.vertex(index))
                .collect(),
        }
    }
}

/// One frame entry of a model held whole (MDL-5.1).
#[derive(Clone, Debug, PartialEq)]
pub enum ModelFrameEntry {
    /// One frame.
    Single(ModelFrame),
    /// A group of frames shown in turn.
    Group {
        /// The stored type, [`super::MDL_FRAME_GROUP`] in a model that is made.
        frame_type: i32,
        /// The group's bounding box's minimum.
        bboxmin: TriVertX,
        /// The group's bounding box's maximum.
        bboxmax: TriVertX,
        /// One interval per frame.
        intervals: Vec<f32>,
        /// The frames.
        frames: Vec<ModelFrame>,
    },
}

/// Why a model cannot be written.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MdlWriteError {
    /// A frame's name is longer than 15 bytes or holds a NUL.
    FrameName,
    /// A skin does not hold `skinwidth * skinheight` pixels.
    SkinSize {
        /// The skin entry.
        skin: usize,
    },
    /// A frame does not hold one vertex for each of the model's.
    FrameVertices {
        /// The frame entry.
        frame: usize,
    },
    /// A group's intervals are not one for each of its skins or frames.
    Intervals,
    /// A count does not fit the file's 32-bit fields.
    TooMany,
    /// What would be written does not load: the loader's own error for it (MDL-6.4).
    Unreadable(MdlError),
}

impl fmt::Display for MdlWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FrameName => {
                f.write_str("a frame's name is at most 15 bytes and holds no NUL")
            }
            Self::SkinSize { skin } => {
                write!(f, "skin {skin} is not the model's skin width by its height")
            }
            Self::FrameVertices { frame } => {
                write!(f, "frame {frame} does not have a vertex for each of the model's")
            }
            Self::Intervals => {
                f.write_str("a group needs one interval for each of its skins or frames")
            }
            Self::TooMany => f.write_str("a count does not fit the file"),
            Self::Unreadable(error) => write!(f, "{}", error.message("the model")),
        }
    }
}

impl core::error::Error for MdlWriteError {}

/// An alias model held whole: every part of the file, owned (MDL-6).
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    /// The header. Its counts are those read; [`Model::to_bytes`] writes the counts of the
    /// parts below.
    pub header: MdlHeader,
    /// The skin entries.
    pub skins: Vec<ModelSkin>,
    /// One texture coordinate per vertex.
    pub stverts: Vec<StVert>,
    /// The triangles.
    pub triangles: Vec<Triangle>,
    /// The frame entries.
    pub frames: Vec<ModelFrameEntry>,
    /// The bytes after the last frame entry, which the engine ignores (MDL-5.7).
    pub trailing: Vec<u8>,
}

/// A count as the file's 32-bit field holds it.
fn count(len: usize) -> Result<i32, MdlWriteError> {
    i32::try_from(len).map_err(|_| MdlWriteError::TooMany)
}

/// Bytes being written.
#[derive(Default)]
struct Out(Vec<u8>);

impl Out {
    fn int(&mut self, value: i32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn float(&mut self, value: f32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn vert(&mut self, vert: TriVertX) {
        self.0.extend_from_slice(&vert.v);
        self.0.push(vert.lightnormalindex);
    }

    /// A frame, id's `WriteFrame`: its `daliasframe_t` and its vertices.
    fn frame(&mut self, frame: &ModelFrame) {
        self.vert(frame.bboxmin);
        self.vert(frame.bboxmax);
        self.0.extend_from_slice(&frame.name);
        for &vert in &frame.verts {
            self.vert(vert);
        }
    }
}

impl Model {
    /// Reads an alias model whole, as [`Mdl::read`] reads it.
    ///
    /// # Errors
    ///
    /// The [`MdlError`] of [`Mdl::read`].
    pub fn read(bytes: &[u8]) -> Result<Self, MdlError> {
        let mdl = Mdl::read(bytes)?;
        let skins = mdl
            .skins
            .iter()
            .map(|skin| match skin {
                MdlSkin::Single(pixels) => ModelSkin::Single(pixels.to_vec()),
                MdlSkin::Group {
                    skin_type,
                    intervals,
                    skins,
                } => ModelSkin::Group {
                    skin_type: *skin_type,
                    intervals: intervals.clone(),
                    skins: skins.iter().map(|pixels| pixels.to_vec()).collect(),
                },
            })
            .collect();
        let frames = mdl
            .frames
            .iter()
            .map(|entry| match entry {
                MdlFrameEntry::Single(frame) => ModelFrameEntry::Single(ModelFrame::of(frame)),
                MdlFrameEntry::Group {
                    frame_type,
                    bboxmin,
                    bboxmax,
                    intervals,
                    frames,
                } => ModelFrameEntry::Group {
                    frame_type: *frame_type,
                    bboxmin: *bboxmin,
                    bboxmax: *bboxmax,
                    intervals: intervals.clone(),
                    frames: frames.iter().map(ModelFrame::of).collect(),
                },
            })
            .collect();
        Ok(Self {
            header: mdl.header,
            skins,
            stverts: mdl.stverts,
            triangles: mdl.triangles,
            frames,
            trailing: bytes.get(mdl.end..).unwrap_or_default().to_vec(),
        })
    }

    /// The model's file, id's `WriteModelFile`: the header, with the counts of the parts
    /// held, then the skins, the texture coordinates, the triangles and the frames.
    ///
    /// Implements MDL-6.
    ///
    /// # Errors
    ///
    /// The [`MdlWriteError`] of parts that do not fit each other, or of a file the loader
    /// would refuse.
    pub fn to_bytes(&self) -> Result<Vec<u8>, MdlWriteError> {
        let header = &self.header;
        let skin_size = usize::try_from(header.skinwidth)
            .ok()
            .zip(usize::try_from(header.skinheight).ok())
            .and_then(|(width, height)| width.checked_mul(height));
        let mut out = Out::default();
        out.0.extend_from_slice(&MDL_MAGIC);
        out.int(MDL_VERSION);
        for value in header
            .scale
            .into_iter()
            .chain(header.scale_origin)
            .chain([header.bounding_radius])
            .chain(header.eye_position)
        {
            out.float(value);
        }
        out.int(count(self.skins.len())?);
        out.int(header.skinwidth);
        out.int(header.skinheight);
        out.int(count(self.stverts.len())?);
        out.int(count(self.triangles.len())?);
        out.int(count(self.frames.len())?);
        out.int(header.synctype);
        out.int(header.flags);
        out.float(header.size);
        for (place, skin) in self.skins.iter().enumerate() {
            let fits = |pixels: &Vec<u8>| Some(pixels.len()) == skin_size;
            match skin {
                ModelSkin::Single(pixels) => {
                    if !fits(pixels) {
                        return Err(MdlWriteError::SkinSize { skin: place });
                    }
                    out.int(MDL_SKIN_SINGLE);
                    out.0.extend_from_slice(pixels);
                }
                ModelSkin::Group {
                    skin_type,
                    intervals,
                    skins,
                } => {
                    if intervals.len() != skins.len() {
                        return Err(MdlWriteError::Intervals);
                    }
                    if !skins.iter().all(fits) {
                        return Err(MdlWriteError::SkinSize { skin: place });
                    }
                    out.int(*skin_type);
                    out.int(count(skins.len())?);
                    for &interval in intervals {
                        out.float(interval);
                    }
                    for pixels in skins {
                        out.0.extend_from_slice(pixels);
                    }
                }
            }
        }
        for stvert in &self.stverts {
            out.int(stvert.onseam);
            out.int(stvert.s);
            out.int(stvert.t);
        }
        for triangle in &self.triangles {
            out.int(triangle.facesfront);
            for vertex in triangle.vertindex {
                out.int(count(vertex)?);
            }
        }
        let numverts = self.stverts.len();
        for (place, entry) in self.frames.iter().enumerate() {
            let whole = |frame: &ModelFrame| frame.verts.len() == numverts;
            match entry {
                ModelFrameEntry::Single(frame) => {
                    if !whole(frame) {
                        return Err(MdlWriteError::FrameVertices { frame: place });
                    }
                    out.int(MDL_FRAME_SINGLE);
                    out.frame(frame);
                }
                ModelFrameEntry::Group {
                    frame_type,
                    bboxmin,
                    bboxmax,
                    intervals,
                    frames,
                } => {
                    if intervals.len() != frames.len() {
                        return Err(MdlWriteError::Intervals);
                    }
                    if !frames.iter().all(whole) {
                        return Err(MdlWriteError::FrameVertices { frame: place });
                    }
                    out.int(*frame_type);
                    out.int(count(frames.len())?);
                    out.vert(*bboxmin);
                    out.vert(*bboxmax);
                    for &interval in intervals {
                        out.float(interval);
                    }
                    for frame in frames {
                        out.frame(frame);
                    }
                }
            }
        }
        // What is written must load: the loader's checks are the writer's (MDL-6.4).
        Mdl::read(&out.0).map_err(MdlWriteError::Unreadable)?;
        out.0.extend_from_slice(&self.trailing);
        Ok(out.0)
    }
}

#[cfg(test)]
mod tests;
