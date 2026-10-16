// SPDX-License-Identifier: GPL-3.0-or-later

//! Test fixtures: a factory that lays out PAK archives as the spec's vectors do, data first
//! and the directory last.

#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap, index or overflow is a failed test"
)]

use crate::{PAK_HEADER_SIZE, PAK_MAGIC, PAK_NAME_SIZE};

/// One entry of a [`PakSpec`].
#[derive(Clone, Debug, Default)]
pub(crate) struct EntrySpec {
    /// The bytes of the 56-byte name field; the rest of the field is NUL.
    pub(crate) name: Vec<u8>,
    /// The entry's data.
    pub(crate) data: Vec<u8>,
}

/// An entry named `name` holding `data`.
pub(crate) fn entry(name: &str, data: &[u8]) -> EntrySpec {
    EntrySpec {
        name: name.as_bytes().to_vec(),
        data: data.to_vec(),
    }
}

/// A PAK archive to build. The default is a valid archive with no entries.
#[derive(Clone, Debug)]
pub(crate) struct PakSpec {
    pub(crate) magic: [u8; 4],
    pub(crate) entries: Vec<EntrySpec>,
    /// Bytes appended to the directory after the last whole entry (PAK-2.2).
    pub(crate) extra_directory: Vec<u8>,
}

impl Default for PakSpec {
    fn default() -> Self {
        Self {
            magic: PAK_MAGIC,
            entries: Vec::new(),
            extra_directory: Vec::new(),
        }
    }
}

impl PakSpec {
    /// The archive's bytes: header, each entry's data in order, then the directory.
    pub(crate) fn build(&self) -> Vec<u8> {
        let mut data = Vec::new();
        let mut directory = Vec::new();
        for spec in &self.entries {
            let filepos = PAK_HEADER_SIZE + data.len();
            data.extend_from_slice(&spec.data);
            let mut name = spec.name.clone();
            name.resize(PAK_NAME_SIZE, 0);
            directory.extend_from_slice(&name);
            directory.extend_from_slice(&i32::try_from(filepos).unwrap().to_le_bytes());
            directory.extend_from_slice(&i32::try_from(spec.data.len()).unwrap().to_le_bytes());
        }
        directory.extend_from_slice(&self.extra_directory);
        let dirofs = PAK_HEADER_SIZE + data.len();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&self.magic);
        bytes.extend_from_slice(&i32::try_from(dirofs).unwrap().to_le_bytes());
        bytes.extend_from_slice(&i32::try_from(directory.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(&data);
        bytes.extend_from_slice(&directory);
        bytes
    }
}

/// Sets the little-endian 32-bit integer at `offset` of `bytes` to `value`.
pub(crate) fn patch(bytes: &mut [u8], offset: usize, value: i32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
