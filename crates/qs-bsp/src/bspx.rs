// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: common/bspfile.cc, LoadBSPFile, write_bspx, transfer
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: include/common/bspxfile.hh, bspx_header_t, bspx_lump_t

//! BSPX: named lumps after a BSP's own (BSP-24), where tools keep what the format has no
//! lump for, as a lightmap's scale by face (`LMSHIFT`).

use crate::write::BspWriteError;
use crate::{BSP_HEADER_LUMPS, LUMP_DIRECTORY, LUMP_ENTRY, LUMP_FILELEN, u32_at};

/// The four bytes a BSPX directory starts with.
const MAGIC: &[u8; 4] = b"BSPX";
/// The bytes of the directory's header, the magic and the count, and of a 32-bit field.
const HEADER: usize = 8;
const FIELD: usize = 4;
/// The bytes of a lump's name, its last a NUL, and of its entry: the name, offset and length.
const NAME: usize = 24;
const ENTRY: usize = 32;
/// What everything in a BSPX directory is aligned to.
const ALIGN: usize = 4;

/// A BSP's BSPX lumps, in the order they were read or set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bspx {
    lumps: Vec<(Vec<u8>, Vec<u8>)>,
}

/// `value` up to the next multiple of four.
const fn aligned(value: usize) -> usize {
    value.saturating_add(ALIGN - 1) & !(ALIGN - 1)
}

impl Bspx {
    /// The BSPX lumps of the BSP file `file` (BSP-24.1 to BSP-24.3): none for a file with no
    /// BSPX directory or a bad one, and those before the first bad entry.
    #[must_use]
    pub fn read(file: &[u8]) -> Self {
        let mut out = Self::default();
        // After the end of the last of the BSP's own lumps, whatever their order.
        let mut end = 0_usize;
        for lump in 0..BSP_HEADER_LUMPS {
            let at = lump
                .saturating_mul(LUMP_ENTRY)
                .saturating_add(LUMP_DIRECTORY);
            let offset = u32_at(file, at);
            let length = u32_at(file, at.saturating_add(LUMP_FILELEN));
            let lump_end = usize::try_from(offset)
                .ok()
                .zip(usize::try_from(length).ok())
                .map_or(usize::MAX, |(offset, length)| offset.saturating_add(length));
            end = end.max(lump_end);
        }
        let start = aligned(end);
        let Some(header) = file.get(start..start.saturating_add(HEADER)) else {
            return out;
        };
        if header.get(..FIELD) != Some(&MAGIC[..]) {
            return out;
        }
        let count = usize::try_from(u32_at(header, FIELD)).unwrap_or(0);
        for index in 0..count {
            let at = start
                .saturating_add(HEADER)
                .saturating_add(index.saturating_mul(ENTRY));
            let Some(entry) = file.get(at..at.saturating_add(ENTRY)) else {
                break;
            };
            let offset = usize::try_from(u32_at(entry, NAME)).unwrap_or(usize::MAX);
            let length =
                usize::try_from(u32_at(entry, NAME.saturating_add(FIELD))).unwrap_or(usize::MAX);
            let Some(data) = file.get(offset..offset.saturating_add(length)) else {
                break;
            };
            let name = entry.get(..NAME).unwrap_or_default();
            let name = name.split(|&b| b == 0).next().unwrap_or_default();
            out.set(name, data.to_vec());
        }
        out
    }

    /// The lump named `name`.
    #[must_use]
    pub fn get(&self, name: &[u8]) -> Option<&[u8]> {
        self.lumps
            .iter()
            .find(|(known, _)| known == name)
            .map(|(_, data)| data.as_slice())
    }

    /// The lump named `name` given `data`, in the place of one of that name or last
    /// (`transfer`).
    pub fn set(&mut self, name: &[u8], data: Vec<u8>) {
        match self.lumps.iter_mut().find(|(known, _)| known == name) {
            Some((_, old)) => *old = data,
            None => self.lumps.push((name.to_vec(), data)),
        }
    }

    /// The lump named `name` taken out.
    pub fn remove(&mut self, name: &[u8]) {
        self.lumps.retain(|(known, _)| known != name);
    }

    /// Whether there is no lump.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.lumps.is_empty()
    }

    /// The names of the lumps, in order.
    pub fn names(&self) -> impl Iterator<Item = &[u8]> {
        self.lumps.iter().map(|(name, _)| name.as_slice())
    }

    /// The lumps written after `file`, a BSP whose own lumps end it (BSP-24.4): padding to a
    /// multiple of four, the directory, then each lump's bytes padded to a multiple of four.
    /// Nothing is written when there is no lump. A name is cut to 23 bytes.
    ///
    /// # Errors
    ///
    /// [`BspWriteError::TooLarge`] when an offset or a length does not fit 32 bits.
    pub fn append_to(&self, file: &mut Vec<u8>) -> Result<(), BspWriteError> {
        if self.lumps.is_empty() {
            return Ok(());
        }
        let field = |value: usize| u32::try_from(value).map_err(|_| BspWriteError::TooLarge);
        file.resize(aligned(file.len()), 0);
        file.extend_from_slice(MAGIC);
        file.extend_from_slice(&field(self.lumps.len())?.to_le_bytes());
        let directory = file.len();
        file.resize(
            directory.saturating_add(self.lumps.len().saturating_mul(ENTRY)),
            0,
        );
        for (index, (name, data)) in self.lumps.iter().enumerate() {
            let offset = field(file.len())?;
            let length = field(data.len())?;
            file.extend_from_slice(data);
            file.resize(aligned(file.len()), 0);
            let at = directory.saturating_add(index.saturating_mul(ENTRY));
            let Some(entry) = file.get_mut(at..at.saturating_add(ENTRY)) else {
                return Err(BspWriteError::TooLarge);
            };
            let kept = name.len().min(NAME - 1);
            for (slot, byte) in entry.iter_mut().zip(name.iter().take(kept)) {
                *slot = *byte;
            }
            for (slot, byte) in entry
                .iter_mut()
                .skip(NAME)
                .zip(offset.to_le_bytes().into_iter().chain(length.to_le_bytes()))
            {
                *slot = byte;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "test code: a failed unwrap or index is a failed test, and the numbers are small"
    )]

    use super::*;
    use crate::BSP_HEADER_SIZE;

    /// A BSP of a header alone, whose last lump ends at `end`, followed by `after`.
    fn file(end: u32, after: &[u8]) -> Vec<u8> {
        let mut file = vec![0_u8; BSP_HEADER_SIZE];
        file[..4].copy_from_slice(&29_u32.to_le_bytes());
        // The third lump's entry: it starts at the header's end and ends at `end`.
        let entry = LUMP_DIRECTORY + 2 * LUMP_ENTRY;
        let header = u32::try_from(BSP_HEADER_SIZE).unwrap();
        file[entry..entry + 4].copy_from_slice(&header.to_le_bytes());
        file[entry + 4..entry + 8].copy_from_slice(&(end - header).to_le_bytes());
        file.resize(usize::try_from(end).unwrap(), 7);
        file.extend_from_slice(after);
        file
    }

    // spec BSP-24.1: the directory is after the end of the last lump, at the next multiple
    // of four, and starts with BSPX and its count
    // spec BSP-24.2: each entry is a name of 24 bytes, an offset and a length
    // spec BSP-24.4: written lumps are read back, each padded to a multiple of four
    #[test]
    fn lumps_written_are_read_back() {
        let mut lumps = Bspx::default();
        lumps.set(b"LMSHIFT", vec![4, 3, 2]);
        lumps.set(b"OTHER", vec![9; 8]);
        lumps.set(b"LMSHIFT", vec![4, 3, 1]);
        assert_eq!(
            lumps.names().collect::<Vec<_>>(),
            [&b"LMSHIFT"[..], b"OTHER"]
        );
        // The BSP's lumps end at 126: the directory starts at 128.
        let mut written = file(126, &[]);
        lumps.append_to(&mut written).unwrap();
        assert_eq!(&written[128..132], b"BSPX");
        assert_eq!(u32_at(&written, 132), 2);
        assert_eq!(&written[136..143], b"LMSHIFT");
        assert_eq!(written[143], 0);
        let first = usize::try_from(u32_at(&written, 136 + 24)).unwrap();
        assert_eq!(first, 136 + 2 * 32);
        assert_eq!(u32_at(&written, 136 + 28), 3);
        assert_eq!(&written[first..first + 4], [4, 3, 1, 0], "padded to four");
        assert_eq!(written.len() % 4, 0);
        let read = Bspx::read(&written);
        assert_eq!(read, lumps);
        assert_eq!(read.get(b"LMSHIFT"), Some(&[4, 3, 1][..]));
        assert_eq!(read.get(b"NONE"), None);
        let mut fewer = read;
        fewer.remove(b"LMSHIFT");
        assert_eq!(fewer.names().collect::<Vec<_>>(), [&b"OTHER"[..]]);
        assert!(!fewer.is_empty());
    }

    // spec BSP-24.3: a file with no directory, a bad magic or a short file has no lumps;
    // an entry past the file ends the reading, those before it kept
    #[test]
    fn a_bad_directory_has_no_lumps() {
        assert!(Bspx::read(&file(124, &[])).is_empty());
        assert!(Bspx::read(&file(124, b"BSPY\x01\0\0\0")).is_empty());
        assert!(Bspx::read(&file(124, b"BSP")).is_empty());
        assert!(Bspx::read(&[]).is_empty());
        let mut lumps = Bspx::default();
        lumps.set(b"A", vec![1]);
        lumps.set(b"B", vec![2]);
        let mut written = file(124, &[]);
        lumps.append_to(&mut written).unwrap();
        // The second entry's length reaches past the file.
        let second = 124 + 8 + 32;
        written[second + 28..second + 32].copy_from_slice(&u32::MAX.to_le_bytes());
        let read = Bspx::read(&written);
        assert_eq!(read.names().collect::<Vec<_>>(), [&b"A"[..]]);
        // A count of more entries than the file holds stops at its end.
        let mut counted = file(124, &[]);
        lumps.append_to(&mut counted).unwrap();
        counted[128..132].copy_from_slice(&1000_u32.to_le_bytes());
        assert!(Bspx::read(&counted).names().count() >= 2);
    }

    // spec BSP-24.4: no lump writes nothing, and a name is cut to 23 bytes
    #[test]
    fn no_lumps_write_nothing_and_a_long_name_is_cut() {
        let mut plain = file(124, &[]);
        Bspx::default().append_to(&mut plain).unwrap();
        assert_eq!(plain.len(), 124);
        let mut lumps = Bspx::default();
        lumps.set(&[b'N'; 30], vec![1]);
        let mut written = file(124, &[]);
        lumps.append_to(&mut written).unwrap();
        let read = Bspx::read(&written);
        assert_eq!(read.names().next().unwrap(), &[b'N'; 23][..]);
    }
}
