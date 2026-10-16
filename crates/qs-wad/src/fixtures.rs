// SPDX-License-Identifier: GPL-3.0-or-later

//! Test fixtures: a factory that lays out WAD2 files as the spec's vectors do, lump data
//! first and the directory last.

#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap, index or overflow is a failed test"
)]

use crate::{WAD_HEADER_SIZE, WAD_MAGIC, WAD_NAME_SIZE, WAD_TYP_QPIC};

/// One lump of a [`WadSpec`].
#[derive(Clone, Debug)]
pub(crate) struct LumpSpec {
    /// The bytes of the 16-byte name field; the rest of the field is NUL.
    pub(crate) name: Vec<u8>,
    pub(crate) lump_type: u8,
    pub(crate) compression: u8,
    pub(crate) data: Vec<u8>,
}

/// An uncompressed picture lump named `name` holding `data`.
pub(crate) fn lump(name: &[u8], data: &[u8]) -> LumpSpec {
    LumpSpec {
        name: name.to_vec(),
        lump_type: WAD_TYP_QPIC,
        compression: 0,
        data: data.to_vec(),
    }
}

/// A WAD2 file to build. The default is a valid file with no lumps.
#[derive(Clone, Debug)]
pub(crate) struct WadSpec {
    pub(crate) magic: [u8; 4],
    pub(crate) lumps: Vec<LumpSpec>,
}

impl Default for WadSpec {
    fn default() -> Self {
        Self {
            magic: WAD_MAGIC,
            lumps: Vec::new(),
        }
    }
}

impl WadSpec {
    /// The file's bytes: header, each lump's data in order, then the directory.
    pub(crate) fn build(&self) -> Vec<u8> {
        let mut data = Vec::new();
        let mut directory = Vec::new();
        for spec in &self.lumps {
            let filepos = i32::try_from(WAD_HEADER_SIZE + data.len()).unwrap();
            let len = i32::try_from(spec.data.len()).unwrap();
            data.extend_from_slice(&spec.data);
            directory.extend_from_slice(&filepos.to_le_bytes());
            directory.extend_from_slice(&len.to_le_bytes());
            directory.extend_from_slice(&len.to_le_bytes());
            directory.extend_from_slice(&[spec.lump_type, spec.compression, 0, 0]);
            let mut name = spec.name.clone();
            name.resize(WAD_NAME_SIZE, 0);
            directory.extend_from_slice(&name);
        }
        let numlumps = i32::try_from(self.lumps.len()).unwrap();
        let infotableofs = i32::try_from(WAD_HEADER_SIZE + data.len()).unwrap();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&self.magic);
        bytes.extend_from_slice(&numlumps.to_le_bytes());
        bytes.extend_from_slice(&infotableofs.to_le_bytes());
        bytes.extend_from_slice(&data);
        bytes.extend_from_slice(&directory);
        bytes
    }
}

/// Sets the little-endian 32-bit integer at `offset` of `bytes` to `value`.
pub(crate) fn patch(bytes: &mut [u8], offset: usize, value: i32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
