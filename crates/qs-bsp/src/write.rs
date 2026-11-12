// SPDX-License-Identifier: GPL-3.0-or-later

//! Writing a BSP29 file (BSP-23): the header, then the lumps in the order id's `WriteBSPFile`
//! writes them, each padded to four bytes, from records laid out as BSP-1 to BSP-15 read them.

use std::fmt;

use crate::{BSP_HEADER_LUMPS, BSP_HEADER_SIZE, BSP_MAXLIGHTMAPS, BSP_NUM_AMBIENTS, BSP_VERSION};

/// The lumps' numbers in the header (BSP-1.1).
const LUMP_ENTITIES: usize = 0;
const LUMP_PLANES: usize = 1;
const LUMP_TEXTURES: usize = 2;
const LUMP_VERTEXES: usize = 3;
const LUMP_VISIBILITY: usize = 4;
const LUMP_NODES: usize = 5;
const LUMP_TEXINFO: usize = 6;
const LUMP_FACES: usize = 7;
const LUMP_LIGHTING: usize = 8;
const LUMP_CLIPNODES: usize = 9;
const LUMP_LEAFS: usize = 10;
const LUMP_MARKSURFACES: usize = 11;
const LUMP_EDGES: usize = 12;
const LUMP_SURFEDGES: usize = 13;
const LUMP_MODELS: usize = 14;
/// What each lump's length is padded to a multiple of (BSP-23.2).
const LUMP_ALIGN: usize = 4;
/// The bytes of a header entry: its offset and length.
const LUMP_ENTRY: usize = 8;

/// A plane as the file stores it (BSP-3.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiskPlane {
    /// The unit normal.
    pub normal: [f32; 3],
    /// The distance from the origin.
    pub dist: f32,
    /// `PLANE_X` to `PLANE_ANYZ` (BSP-3.2).
    pub plane_type: i32,
}

/// A node as the file stores it (BSP-7.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiskNode {
    /// Its plane.
    pub planenum: i32,
    /// Its children: a node, or `-(leaf + 1)` (BSP-7.2).
    pub children: [i16; 2],
    /// Its bounds, in whole units.
    pub mins: [i16; 3],
    /// Its bounds, in whole units.
    pub maxs: [i16; 3],
    /// Its first face.
    pub firstface: u16,
    /// Its faces.
    pub numfaces: u16,
}

/// A clip node as the file stores it (BSP-11.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiskClipNode {
    /// Its plane.
    pub planenum: i32,
    /// Its children: a clip node, or a contents value.
    pub children: [i16; 2],
}

/// A texinfo as the file stores it (BSP-8.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiskTexinfo {
    /// The s and t axes and offsets.
    pub vecs: [[f32; 4]; 2],
    /// The texture slot.
    pub miptex: i32,
    /// `TEX_SPECIAL` or 0.
    pub flags: i32,
}

/// A face as the file stores it (BSP-9.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiskFace {
    /// Its plane.
    pub planenum: i16,
    /// 1 when it faces away from its plane.
    pub side: i16,
    /// Its first surfedge.
    pub firstedge: i32,
    /// Its surfedges.
    pub numedges: i16,
    /// Its texinfo.
    pub texinfo: i16,
    /// Its light styles, 255 for unused slots.
    pub styles: [u8; BSP_MAXLIGHTMAPS],
    /// Its lightmaps' offset, -1 for none.
    pub lightofs: i32,
}

/// A leaf as the file stores it (BSP-12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiskLeaf {
    /// `CONTENTS_` value.
    pub contents: i32,
    /// Its visibility row's offset, -1 for none.
    pub visofs: i32,
    /// Its bounds, in whole units.
    pub mins: [i16; 3],
    /// Its bounds, in whole units.
    pub maxs: [i16; 3],
    /// Its first marksurface.
    pub firstmarksurface: u16,
    /// Its marksurfaces.
    pub nummarksurfaces: u16,
    /// The ambient sound levels.
    pub ambient_level: [u8; BSP_NUM_AMBIENTS],
}

/// A model as the file stores it (BSP-15).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DiskModel {
    /// Its bounds.
    pub mins: [f32; 3],
    /// Its bounds.
    pub maxs: [f32; 3],
    /// Its origin.
    pub origin: [f32; 3],
    /// The head node of each hull.
    pub headnode: [i32; 4],
    /// Its leaves, the shared solid leaf not counted.
    pub visleafs: i32,
    /// Its first face.
    pub firstface: i32,
    /// Its faces.
    pub numfaces: i32,
}

/// Why a BSP could not be written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BspWriteError {
    /// The file would be larger than a signed 32-bit offset reaches (BSP-23.3).
    TooLarge,
}

impl fmt::Display for BspWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => f.write_str("the BSP would be larger than 2 GiB"),
        }
    }
}

impl core::error::Error for BspWriteError {}

/// Every lump of a BSP29 file, as records.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BspFile {
    /// The entity text, its NUL included (BSP-2).
    pub entities: Vec<u8>,
    /// The planes.
    pub planes: Vec<DiskPlane>,
    /// The textures lump's bytes (BSP-4).
    pub textures: Vec<u8>,
    /// The vertexes.
    pub vertexes: Vec<[f32; 3]>,
    /// The visibility lump's bytes (BSP-6).
    pub visibility: Vec<u8>,
    /// The nodes.
    pub nodes: Vec<DiskNode>,
    /// The texinfo.
    pub texinfo: Vec<DiskTexinfo>,
    /// The faces.
    pub faces: Vec<DiskFace>,
    /// The lighting lump's bytes (BSP-10).
    pub lighting: Vec<u8>,
    /// The clip nodes.
    pub clipnodes: Vec<DiskClipNode>,
    /// The leaves.
    pub leafs: Vec<DiskLeaf>,
    /// The marksurfaces.
    pub marksurfaces: Vec<u16>,
    /// The edges, each two vertexes.
    pub edges: Vec<[u16; 2]>,
    /// The surfedges.
    pub surfedges: Vec<i32>,
    /// The models.
    pub models: Vec<DiskModel>,
}

/// Little-endian records.
trait Record {
    fn put(&self, out: &mut Vec<u8>);
}

fn floats(values: &[f32], out: &mut Vec<u8>) {
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

fn shorts(values: &[i16], out: &mut Vec<u8>) {
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

impl Record for DiskPlane {
    fn put(&self, out: &mut Vec<u8>) {
        floats(&self.normal, out);
        floats(&[self.dist], out);
        out.extend_from_slice(&self.plane_type.to_le_bytes());
    }
}

impl Record for [f32; 3] {
    fn put(&self, out: &mut Vec<u8>) {
        floats(self, out);
    }
}

impl Record for DiskNode {
    fn put(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.planenum.to_le_bytes());
        shorts(&self.children, out);
        shorts(&self.mins, out);
        shorts(&self.maxs, out);
        out.extend_from_slice(&self.firstface.to_le_bytes());
        out.extend_from_slice(&self.numfaces.to_le_bytes());
    }
}

impl Record for DiskTexinfo {
    fn put(&self, out: &mut Vec<u8>) {
        for axis in &self.vecs {
            floats(axis, out);
        }
        out.extend_from_slice(&self.miptex.to_le_bytes());
        out.extend_from_slice(&self.flags.to_le_bytes());
    }
}

impl Record for DiskFace {
    fn put(&self, out: &mut Vec<u8>) {
        shorts(&[self.planenum, self.side], out);
        out.extend_from_slice(&self.firstedge.to_le_bytes());
        shorts(&[self.numedges, self.texinfo], out);
        out.extend_from_slice(&self.styles);
        out.extend_from_slice(&self.lightofs.to_le_bytes());
    }
}

impl Record for DiskClipNode {
    fn put(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.planenum.to_le_bytes());
        shorts(&self.children, out);
    }
}

impl Record for DiskLeaf {
    fn put(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.contents.to_le_bytes());
        out.extend_from_slice(&self.visofs.to_le_bytes());
        shorts(&self.mins, out);
        shorts(&self.maxs, out);
        out.extend_from_slice(&self.firstmarksurface.to_le_bytes());
        out.extend_from_slice(&self.nummarksurfaces.to_le_bytes());
        out.extend_from_slice(&self.ambient_level);
    }
}

impl Record for u16 {
    fn put(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.to_le_bytes());
    }
}

impl Record for [u16; 2] {
    fn put(&self, out: &mut Vec<u8>) {
        for value in self {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
}

impl Record for i32 {
    fn put(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.to_le_bytes());
    }
}

impl Record for DiskModel {
    fn put(&self, out: &mut Vec<u8>) {
        floats(&self.mins, out);
        floats(&self.maxs, out);
        floats(&self.origin, out);
        for head in &self.headnode {
            out.extend_from_slice(&head.to_le_bytes());
        }
        for value in [self.visleafs, self.firstface, self.numfaces] {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
}

fn records<T: Record>(items: &[T]) -> Vec<u8> {
    let mut out = Vec::new();
    for item in items {
        item.put(&mut out);
    }
    out
}

impl BspFile {
    /// The file's bytes.
    ///
    /// Implements BSP-23: version 29, then planes, leafs, vertexes, nodes, texinfo, faces,
    /// clipnodes, marksurfaces, surfedges, edges, models, lighting, visibility, entities and
    /// textures, each lump's bytes followed by NUL bytes to a multiple of four.
    ///
    /// # Errors
    ///
    /// [`BspWriteError::TooLarge`] when an offset or length would not fit 32 signed bits.
    pub fn to_bytes(&self) -> Result<Vec<u8>, BspWriteError> {
        let lumps: [(usize, Vec<u8>); BSP_HEADER_LUMPS] = [
            (LUMP_PLANES, records(&self.planes)),
            (LUMP_LEAFS, records(&self.leafs)),
            (LUMP_VERTEXES, records(&self.vertexes)),
            (LUMP_NODES, records(&self.nodes)),
            (LUMP_TEXINFO, records(&self.texinfo)),
            (LUMP_FACES, records(&self.faces)),
            (LUMP_CLIPNODES, records(&self.clipnodes)),
            (LUMP_MARKSURFACES, records(&self.marksurfaces)),
            (LUMP_SURFEDGES, records(&self.surfedges)),
            (LUMP_EDGES, records(&self.edges)),
            (LUMP_MODELS, records(&self.models)),
            (LUMP_LIGHTING, self.lighting.clone()),
            (LUMP_VISIBILITY, self.visibility.clone()),
            (LUMP_ENTITIES, self.entities.clone()),
            (LUMP_TEXTURES, self.textures.clone()),
        ];
        let mut out = vec![0_u8; BSP_HEADER_SIZE];
        if let Some(version) = out.get_mut(..LUMP_ALIGN) {
            version.copy_from_slice(&BSP_VERSION.to_le_bytes());
        }
        for (number, data) in lumps {
            let offset = offset_field(out.len())?;
            let length = offset_field(data.len())?;
            out.extend_from_slice(&data);
            while !out.len().is_multiple_of(LUMP_ALIGN) {
                out.push(0);
            }
            let entry = number
                .checked_mul(LUMP_ENTRY)
                .and_then(|at| at.checked_add(LUMP_ALIGN))
                .ok_or(BspWriteError::TooLarge)?;
            if let Some(slot) = out.get_mut(entry..entry.saturating_add(LUMP_ENTRY)) {
                let mut both = [0_u8; LUMP_ENTRY];
                for (to, from) in both
                    .iter_mut()
                    .zip(offset.to_le_bytes().into_iter().chain(length.to_le_bytes()))
                {
                    *to = from;
                }
                slot.copy_from_slice(&both);
            }
        }
        offset_field(out.len())?;
        Ok(out)
    }
}

/// A size or offset as the file's signed 32-bit field (BSP-23.3).
fn offset_field(value: usize) -> Result<i32, BspWriteError> {
    i32::try_from(value).map_err(|_| BspWriteError::TooLarge)
}

#[cfg(test)]
mod tests;
