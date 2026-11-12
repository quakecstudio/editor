// SPDX-License-Identifier: GPL-3.0-or-later

//! Writing a BSP29 or `BSP2` file (BSP-23): the header, then the lumps in the order id's
//! `WriteBSPFile` writes them, each padded to four bytes, from records laid out as BSP-1 to
//! BSP-15 read them, or BSP-17's for `BSP2`.

use std::fmt;

use crate::{
    BSP_BSP2_VERSION, BSP_HEADER_LUMPS, BSP_HEADER_SIZE, BSP_MAXLIGHTMAPS, BSP_NUM_AMBIENTS,
    BSP_VERSION, LumpId,
};

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

/// The layout a file is written in (BSP-23.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BspFormat {
    /// Version 29, id's (BSP-1 to BSP-15).
    #[default]
    Bsp29,
    /// `BSP2`: 32-bit fields and float bounds (BSP-17).
    Bsp2,
}

/// A node (BSP-7.1, BSP-17), its fields as wide as `BSP2`'s.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiskNode {
    /// Its plane.
    pub planenum: i32,
    /// Its children: a node, or `-(leaf + 1)` (BSP-7.2).
    pub children: [i32; 2],
    /// Its bounds; BSP29 stores each as a 16-bit integer, truncated toward zero.
    pub mins: [f32; 3],
    /// Its bounds; BSP29 stores each as a 16-bit integer, truncated toward zero.
    pub maxs: [f32; 3],
    /// Its first face.
    pub firstface: u32,
    /// Its faces.
    pub numfaces: u32,
}

/// A clip node (BSP-11.1, BSP-17).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiskClipNode {
    /// Its plane.
    pub planenum: i32,
    /// Its children: a clip node, or a contents value.
    pub children: [i32; 2],
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

/// A face (BSP-9.1, BSP-17).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiskFace {
    /// Its plane.
    pub planenum: i32,
    /// 1 when it faces away from its plane.
    pub side: i32,
    /// Its first surfedge.
    pub firstedge: i32,
    /// Its surfedges.
    pub numedges: i32,
    /// Its texinfo.
    pub texinfo: i32,
    /// Its light styles, 255 for unused slots.
    pub styles: [u8; BSP_MAXLIGHTMAPS],
    /// Its lightmaps' offset, -1 for none.
    pub lightofs: i32,
}

/// A leaf (BSP-12, BSP-17).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiskLeaf {
    /// `CONTENTS_` value.
    pub contents: i32,
    /// Its visibility row's offset, -1 for none.
    pub visofs: i32,
    /// Its bounds; BSP29 stores each as a 16-bit integer, truncated toward zero.
    pub mins: [f32; 3],
    /// Its bounds; BSP29 stores each as a 16-bit integer, truncated toward zero.
    pub maxs: [f32; 3],
    /// Its first marksurface.
    pub firstmarksurface: u32,
    /// Its marksurfaces.
    pub nummarksurfaces: u32,
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
    /// A value of the lump does not fit BSP29's 16-bit field (BSP-23.5).
    TooWide(LumpId),
}

impl fmt::Display for BspWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => f.write_str("the BSP would be larger than 2 GiB"),
            Self::TooWide(lump) => {
                write!(
                    f,
                    "a value of the {lump:?} lump does not fit BSP29's 16 bits"
                )
            }
        }
    }
}

impl core::error::Error for BspWriteError {}

/// Every lump of a BSP file, as records.
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
    pub marksurfaces: Vec<u32>,
    /// The edges, each two vertexes.
    pub edges: Vec<[u32; 2]>,
    /// The surfedges.
    pub surfedges: Vec<i32>,
    /// The models.
    pub models: Vec<DiskModel>,
}

/// Little-endian records in one of the formats.
trait Record {
    /// Appends the record; BSP29's narrower fields refuse a value they cannot hold.
    fn put(&self, format: BspFormat, out: &mut Vec<u8>) -> Result<(), BspWriteError>;
}

/// The bytes a record is written into, in a format, with the lump to blame.
struct Put<'a> {
    out: &'a mut Vec<u8>,
    format: BspFormat,
    lump: LumpId,
}

impl Put<'_> {
    fn bytes(&mut self, bytes: &[u8]) {
        self.out.extend_from_slice(bytes);
    }

    fn floats(&mut self, values: &[f32]) {
        for value in values {
            self.bytes(&value.to_le_bytes());
        }
    }

    fn ints(&mut self, values: &[i32]) {
        for value in values {
            self.bytes(&value.to_le_bytes());
        }
    }

    /// A signed field: 16 bits in BSP29, 32 in `BSP2` (BSP-17.2).
    fn signed(&mut self, values: &[i32]) -> Result<(), BspWriteError> {
        for &value in values {
            match self.format {
                BspFormat::Bsp29 => {
                    let narrow =
                        i16::try_from(value).map_err(|_| BspWriteError::TooWide(self.lump))?;
                    self.bytes(&narrow.to_le_bytes());
                }
                BspFormat::Bsp2 => self.bytes(&value.to_le_bytes()),
            }
        }
        Ok(())
    }

    /// An unsigned field: 16 bits in BSP29, 32 in `BSP2` (BSP-17.3).
    fn unsigned(&mut self, values: &[u32]) -> Result<(), BspWriteError> {
        for &value in values {
            match self.format {
                BspFormat::Bsp29 => {
                    let narrow =
                        u16::try_from(value).map_err(|_| BspWriteError::TooWide(self.lump))?;
                    self.bytes(&narrow.to_le_bytes());
                }
                BspFormat::Bsp2 => self.bytes(&value.to_le_bytes()),
            }
        }
        Ok(())
    }

    /// Node or leaf bounds: 16-bit integers truncated toward zero in BSP29, refused when the
    /// truncated value does not fit, floats in `BSP2` (BSP-17.4, BSP-23.5).
    #[expect(
        clippy::cast_possible_truncation,
        reason = "spec BSP-23.5: BSP29 bounds are the floats assigned to shorts, toward zero"
    )]
    fn bounds(&mut self, values: &[f32]) -> Result<(), BspWriteError> {
        let range = f32::from(i16::MIN)..=f32::from(i16::MAX);
        for &value in values {
            match self.format {
                BspFormat::Bsp29 if range.contains(&value.trunc()) => {
                    self.bytes(&(value as i16).to_le_bytes());
                }
                BspFormat::Bsp29 => return Err(BspWriteError::TooWide(self.lump)),
                BspFormat::Bsp2 => self.bytes(&value.to_le_bytes()),
            }
        }
        Ok(())
    }
}

impl Record for DiskPlane {
    fn put(&self, format: BspFormat, out: &mut Vec<u8>) -> Result<(), BspWriteError> {
        let mut put = Put {
            out,
            format,
            lump: LumpId::Planes,
        };
        put.floats(&self.normal);
        put.floats(&[self.dist]);
        put.ints(&[self.plane_type]);
        Ok(())
    }
}

impl Record for [f32; 3] {
    fn put(&self, format: BspFormat, out: &mut Vec<u8>) -> Result<(), BspWriteError> {
        Put {
            out,
            format,
            lump: LumpId::Vertexes,
        }
        .floats(self);
        Ok(())
    }
}

impl Record for DiskNode {
    fn put(&self, format: BspFormat, out: &mut Vec<u8>) -> Result<(), BspWriteError> {
        let mut put = Put {
            out,
            format,
            lump: LumpId::Nodes,
        };
        put.ints(&[self.planenum]);
        put.signed(&self.children)?;
        put.bounds(&self.mins)?;
        put.bounds(&self.maxs)?;
        put.unsigned(&[self.firstface, self.numfaces])
    }
}

impl Record for DiskTexinfo {
    fn put(&self, format: BspFormat, out: &mut Vec<u8>) -> Result<(), BspWriteError> {
        let mut put = Put {
            out,
            format,
            lump: LumpId::Texinfo,
        };
        for axis in &self.vecs {
            put.floats(axis);
        }
        put.ints(&[self.miptex, self.flags]);
        Ok(())
    }
}

impl Record for DiskFace {
    fn put(&self, format: BspFormat, out: &mut Vec<u8>) -> Result<(), BspWriteError> {
        let mut put = Put {
            out,
            format,
            lump: LumpId::Faces,
        };
        put.signed(&[self.planenum, self.side])?;
        put.ints(&[self.firstedge]);
        put.signed(&[self.numedges, self.texinfo])?;
        put.bytes(&self.styles);
        put.ints(&[self.lightofs]);
        Ok(())
    }
}

impl Record for DiskClipNode {
    fn put(&self, format: BspFormat, out: &mut Vec<u8>) -> Result<(), BspWriteError> {
        let mut put = Put {
            out,
            format,
            lump: LumpId::Clipnodes,
        };
        put.ints(&[self.planenum]);
        put.signed(&self.children)
    }
}

impl Record for DiskLeaf {
    fn put(&self, format: BspFormat, out: &mut Vec<u8>) -> Result<(), BspWriteError> {
        let mut put = Put {
            out,
            format,
            lump: LumpId::Leafs,
        };
        put.ints(&[self.contents, self.visofs]);
        put.bounds(&self.mins)?;
        put.bounds(&self.maxs)?;
        put.unsigned(&[self.firstmarksurface, self.nummarksurfaces])?;
        put.bytes(&self.ambient_level);
        Ok(())
    }
}

/// A marksurface entry.
struct Marksurface(u32);

impl Record for Marksurface {
    fn put(&self, format: BspFormat, out: &mut Vec<u8>) -> Result<(), BspWriteError> {
        Put {
            out,
            format,
            lump: LumpId::Marksurfaces,
        }
        .unsigned(&[self.0])
    }
}

impl Record for [u32; 2] {
    fn put(&self, format: BspFormat, out: &mut Vec<u8>) -> Result<(), BspWriteError> {
        Put {
            out,
            format,
            lump: LumpId::Edges,
        }
        .unsigned(self)
    }
}

impl Record for i32 {
    fn put(&self, format: BspFormat, out: &mut Vec<u8>) -> Result<(), BspWriteError> {
        Put {
            out,
            format,
            lump: LumpId::Surfedges,
        }
        .ints(&[*self]);
        Ok(())
    }
}

impl Record for DiskModel {
    fn put(&self, format: BspFormat, out: &mut Vec<u8>) -> Result<(), BspWriteError> {
        let mut put = Put {
            out,
            format,
            lump: LumpId::Models,
        };
        put.floats(&self.mins);
        put.floats(&self.maxs);
        put.floats(&self.origin);
        put.ints(&self.headnode);
        put.ints(&[self.visleafs, self.firstface, self.numfaces]);
        Ok(())
    }
}

fn records<T: Record>(items: &[T], format: BspFormat) -> Result<Vec<u8>, BspWriteError> {
    let mut out = Vec::new();
    for item in items {
        item.put(format, &mut out)?;
    }
    Ok(out)
}

impl BspFile {
    /// The file's bytes in `format`.
    ///
    /// Implements BSP-23: the version, then planes, leafs, vertexes, nodes, texinfo, faces,
    /// clipnodes, marksurfaces, surfedges, edges, models, lighting, visibility, entities and
    /// textures, each lump's bytes followed by NUL bytes to a multiple of four.
    ///
    /// # Errors
    ///
    /// [`BspWriteError::TooLarge`] when an offset or length would not fit 32 signed bits, and
    /// [`BspWriteError::TooWide`] when a BSP29 field cannot hold its value.
    pub fn to_bytes(&self, format: BspFormat) -> Result<Vec<u8>, BspWriteError> {
        let marksurfaces: Vec<Marksurface> =
            self.marksurfaces.iter().map(|&m| Marksurface(m)).collect();
        let lumps: [(usize, Vec<u8>); BSP_HEADER_LUMPS] = [
            (LUMP_PLANES, records(&self.planes, format)?),
            (LUMP_LEAFS, records(&self.leafs, format)?),
            (LUMP_VERTEXES, records(&self.vertexes, format)?),
            (LUMP_NODES, records(&self.nodes, format)?),
            (LUMP_TEXINFO, records(&self.texinfo, format)?),
            (LUMP_FACES, records(&self.faces, format)?),
            (LUMP_CLIPNODES, records(&self.clipnodes, format)?),
            (LUMP_MARKSURFACES, records(&marksurfaces, format)?),
            (LUMP_SURFEDGES, records(&self.surfedges, format)?),
            (LUMP_EDGES, records(&self.edges, format)?),
            (LUMP_MODELS, records(&self.models, format)?),
            (LUMP_LIGHTING, self.lighting.clone()),
            (LUMP_VISIBILITY, self.visibility.clone()),
            (LUMP_ENTITIES, self.entities.clone()),
            (LUMP_TEXTURES, self.textures.clone()),
        ];
        let mut out = vec![0_u8; BSP_HEADER_SIZE];
        if let Some(version) = out.get_mut(..LUMP_ALIGN) {
            let number = match format {
                BspFormat::Bsp29 => BSP_VERSION,
                BspFormat::Bsp2 => BSP_BSP2_VERSION,
            };
            version.copy_from_slice(&number.to_le_bytes());
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
