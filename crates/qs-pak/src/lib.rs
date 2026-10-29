// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/common.c, COM_LoadPackFile, COM_FindFile

//! Reads PAK archives, the files Quake keeps its game data in.
//!
//! [`Pak::read`] parses an archive held in memory and [`Pak::read_from`] one in a seekable
//! file, reading only its header and directory. Both are id's `COM_LoadPackFile` with the
//! bounds checks id's engine leaves out; malformed input is a [`PakError`], never a panic.
//! [`Pak::find`] is the archive half of id's `COM_FindFile`, and [`Pak::entry_data`] and
//! [`Pak::read_entry`] return an entry's bytes.
//!
//! The format is specified in `docs/spec/pak.md` (PAK-1 to PAK-5).

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tests;

use core::fmt;
use core::ops::Range;
use std::io::{self, Read, Seek, SeekFrom};

use qc_progs::crc::{PROGS_CRC_INIT, PROGS_CRC_POLY, crc_block};

/// The four bytes a PAK file starts with (PAK-1.1).
pub const PAK_MAGIC: [u8; 4] = *b"PACK";

/// Bytes in the header, id's `sizeof(dpackheader_t)` (PAK-1.1).
pub const PAK_HEADER_SIZE: usize = 12;

/// Bytes in one directory entry, id's `sizeof(dpackfile_t)` (PAK-2.1).
pub const PAK_ENTRY_SIZE: usize = 64;

/// Bytes in an entry's name field, id's `dpackfile_t.name` (PAK-2.1).
pub const PAK_NAME_SIZE: usize = 56;

/// The most entries an archive may have, id's `MAX_FILES_IN_PACK` (PAK-2.4).
pub const PAK_MAX_FILES: u32 = 2048;

/// The entry count of id's original `id1/pak0.pak`, id's `PAK0_COUNT` (PAK-4.2).
pub const PAK_PAK0_COUNT: usize = 339;

/// The directory CRC of id's original `id1/pak0.pak`, id's `PAK0_CRC` (PAK-4.2).
pub const PAK_PAK0_CRC: u16 = 32981;

/// The value the directory CRC starts from, id's `CRC_INIT_VALUE` (PAK-4.1).
pub const PAK_CRC_INIT: u16 = PROGS_CRC_INIT;

/// The directory CRC's polynomial, from the comment above id's `crctable` (PAK-4.1).
pub const PAK_CRC_POLY: u16 = PROGS_CRC_POLY;

/// [`PAK_ENTRY_SIZE`] as the type the entry count is computed in.
const ENTRY_SIZE_U32: u32 = 64;

/// The largest directory, in bytes, that fits id's stack array of [`PAK_MAX_FILES`] entries
/// (PAK-2.6).
const MAX_DIRLEN: u32 = PAK_MAX_FILES.saturating_mul(ENTRY_SIZE_U32);

/// Why an archive or one of its entries could not be read.
///
/// The message is written to follow the archive's path, as id's errors are:
/// `format!("{path} {error}")` gives `id1/pak0.pak is not a packfile`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PakError {
    /// The file does not start with `PACK`, or is shorter than its 12-byte header
    /// (PAK-1.2, PAK-1.3).
    NotAPackfile,
    /// The directory has more than [`PAK_MAX_FILES`] entries, counted as id's 32-bit build
    /// counts them (PAK-2.4, PAK-2.5).
    TooManyFiles {
        /// The computed entry count.
        count: u32,
    },
    /// `dirlen` is 2048 whole entries plus part of another, which overflows id's directory
    /// buffer (PAK-2.6).
    DirectoryTooLarge {
        /// The stored `dirlen`.
        dirlen: i32,
    },
    /// The directory does not lie wholly inside the file (PAK-2.7).
    DirectoryOutOfBounds {
        /// The stored `dirofs`.
        dirofs: i32,
        /// The stored `dirlen`.
        dirlen: i32,
    },
    /// An entry's 56-byte name field has no NUL (PAK-2.9).
    UnterminatedName {
        /// The entry's index in the directory.
        index: usize,
    },
    /// An entry's data does not lie wholly inside the file (PAK-3.4).
    EntryOutOfBounds {
        /// The entry's `filepos`.
        filepos: i32,
        /// The entry's `filelen`.
        filelen: i32,
    },
    /// The file could not be read.
    Io(io::ErrorKind),
}

impl fmt::Display for PakError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAPackfile => f.write_str("is not a packfile"),
            Self::TooManyFiles { count } => write!(f, "has {count} files"),
            Self::DirectoryTooLarge { dirlen } => write!(
                f,
                "has a {dirlen}-byte directory, more than {PAK_MAX_FILES} entries can fill"
            ),
            Self::DirectoryOutOfBounds { dirofs, dirlen } => write!(
                f,
                "has a {dirlen}-byte directory at offset {dirofs}, outside the file"
            ),
            Self::UnterminatedName { index } => {
                write!(f, "has an entry ({index}) whose name has no NUL")
            }
            Self::EntryOutOfBounds { filepos, filelen } => write!(
                f,
                "has an entry of {filelen} bytes at offset {filepos}, outside the file"
            ),
            Self::Io(kind) => write!(f, "could not be read: {kind}"),
        }
    }
}

impl core::error::Error for PakError {}

impl From<io::Error> for PakError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.kind())
    }
}

/// The 12-byte header, id's `dpackheader_t` after `LittleLong`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Header {
    /// Offset of the directory from the start of the file.
    pub dirofs: i32,
    /// Length of the directory in bytes.
    pub dirlen: i32,
}

impl Header {
    /// Decodes the header from the start of `bytes`.
    ///
    /// Implements PAK-1: the magic must be exactly `PACK` (PAK-1.2), and fewer than 12 bytes
    /// is the same error (PAK-1.3).
    ///
    /// # Errors
    ///
    /// [`PakError::NotAPackfile`] when the magic is wrong or `bytes` is too short.
    pub fn parse(bytes: &[u8]) -> Result<Self, PakError> {
        let Some(header) = bytes.get(..PAK_HEADER_SIZE) else {
            return Err(PakError::NotAPackfile);
        };
        // One 32-bit comparison, which coverage-guided fuzzers can solve.
        if le_i32(header, 0) != i32::from_le_bytes(PAK_MAGIC) {
            return Err(PakError::NotAPackfile);
        }
        Ok(Self {
            dirofs: le_i32(header, 4),
            dirlen: le_i32(header, 8),
        })
    }

    /// The number of directory entries, id's `header.dirlen / sizeof(dpackfile_t)`.
    ///
    /// Implements PAK-2.2 and PAK-2.5: the division is unsigned and 32 bits wide, as on
    /// id's reference build, so a negative `dirlen` gives a huge count.
    #[must_use]
    pub const fn entry_count(&self) -> u32 {
        self.dirlen.cast_unsigned().div_euclid(ENTRY_SIZE_U32)
    }

    /// Checks the directory's size and place in a file of `file_len` bytes and returns its
    /// byte range, in the order id's `COM_LoadPackFile` checks.
    ///
    /// Implements PAK-2.4 to PAK-2.7.
    ///
    /// # Errors
    ///
    /// [`PakError::TooManyFiles`], [`PakError::DirectoryTooLarge`] or
    /// [`PakError::DirectoryOutOfBounds`].
    pub fn directory_range(&self, file_len: u64) -> Result<Range<u64>, PakError> {
        let count = self.entry_count();
        if count > PAK_MAX_FILES {
            return Err(PakError::TooManyFiles { count });
        }
        let dirlen = self.dirlen.cast_unsigned();
        if dirlen > MAX_DIRLEN {
            return Err(PakError::DirectoryTooLarge {
                dirlen: self.dirlen,
            });
        }
        let out_of_bounds = PakError::DirectoryOutOfBounds {
            dirofs: self.dirofs,
            dirlen: self.dirlen,
        };
        let Ok(start) = u64::try_from(self.dirofs) else {
            return Err(out_of_bounds);
        };
        let end = start.saturating_add(u64::from(dirlen));
        if end > file_len {
            return Err(out_of_bounds);
        }
        Ok(start..end)
    }
}

/// One directory entry, id's `packfile_t`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Entry {
    name: Vec<u8>,
    /// Offset of the entry's data from the start of the archive.
    pub filepos: i32,
    /// Length of the entry's data in bytes.
    pub filelen: i32,
}

impl Entry {
    /// The entry's name: the raw bytes of its name field before the first NUL, with no case
    /// folding or separator conversion (PAK-2.8, PAK-2.10).
    #[must_use]
    pub fn name(&self) -> &[u8] {
        &self.name
    }

    /// The byte range of the entry's data in an archive of `file_len` bytes.
    ///
    /// Implements PAK-3.2 and PAK-3.4.
    ///
    /// # Errors
    ///
    /// [`PakError::EntryOutOfBounds`] when `filepos` or `filelen` is negative or the data
    /// runs past the end of the file.
    pub fn range(&self, file_len: u64) -> Result<Range<u64>, PakError> {
        let out_of_bounds = PakError::EntryOutOfBounds {
            filepos: self.filepos,
            filelen: self.filelen,
        };
        let (Ok(start), Ok(len)) = (u64::try_from(self.filepos), u64::try_from(self.filelen))
        else {
            return Err(out_of_bounds);
        };
        let end = start.saturating_add(len);
        if end > file_len {
            return Err(out_of_bounds);
        }
        Ok(start..end)
    }
}

/// A loaded archive: its header, its directory and the length of the file it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pak {
    header: Header,
    entries: Vec<Entry>,
    directory_crc: u16,
    file_len: u64,
}

impl Pak {
    /// Loads an archive held in memory, id's `COM_LoadPackFile`.
    ///
    /// Implements PAK-1 and PAK-2, and computes the directory CRC of PAK-4.1. Entry ranges
    /// are not checked (PAK-2.12).
    ///
    /// # Errors
    ///
    /// Any [`PakError`] but [`PakError::EntryOutOfBounds`] and [`PakError::Io`].
    pub fn read(bytes: &[u8]) -> Result<Self, PakError> {
        let header = Header::parse(bytes)?;
        let file_len = len_u64(bytes.len());
        let range = header.directory_range(file_len)?;
        let directory = bytes
            .get(to_usize(range.start)?..to_usize(range.end)?)
            .ok_or(PakError::DirectoryOutOfBounds {
                dirofs: header.dirofs,
                dirlen: header.dirlen,
            })?;
        Self::with_directory(header, directory, file_len)
    }

    /// Loads an archive from a seekable file, reading only its header and directory, id's
    /// `COM_LoadPackFile`.
    ///
    /// Implements PAK-1 and PAK-2 as [`Pak::read`] does; the file's length is its length
    /// when loaded.
    ///
    /// # Errors
    ///
    /// As [`Pak::read`], and [`PakError::Io`] when the file cannot be read.
    pub fn read_from<R: Read + Seek>(reader: &mut R) -> Result<Self, PakError> {
        let file_len = reader.seek(SeekFrom::End(0))?;
        if file_len < len_u64(PAK_HEADER_SIZE) {
            return Err(PakError::NotAPackfile);
        }
        reader.seek(SeekFrom::Start(0))?;
        let mut header = [0_u8; PAK_HEADER_SIZE];
        reader.read_exact(&mut header)?;
        let header = Header::parse(&header)?;
        let range = header.directory_range(file_len)?;
        let directory = read_range(reader, range)?;
        Self::with_directory(header, &directory, file_len)
    }

    /// Parses the `dirlen` bytes of the directory.
    fn with_directory(header: Header, directory: &[u8], file_len: u64) -> Result<Self, PakError> {
        let mut entries = Vec::new();
        for (index, raw) in directory.as_chunks::<PAK_ENTRY_SIZE>().0.iter().enumerate() {
            let field = raw.get(..PAK_NAME_SIZE).unwrap_or_default();
            let Some(nul) = field.iter().position(|&byte| byte == 0) else {
                return Err(PakError::UnterminatedName { index });
            };
            entries.push(Entry {
                name: field.get(..nul).unwrap_or_default().to_vec(),
                filepos: le_i32(raw, PAK_NAME_SIZE),
                filelen: le_i32(raw, PAK_NAME_SIZE.saturating_add(4)),
            });
        }
        Ok(Self {
            header,
            entries,
            directory_crc: crc_block(directory),
            file_len,
        })
    }

    /// The archive's header.
    #[must_use]
    pub const fn header(&self) -> &Header {
        &self.header
    }

    /// The entries, in directory order, duplicates included (PAK-2.11).
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The length of the archive's file when it was loaded.
    #[must_use]
    pub const fn file_len(&self) -> u64 {
        self.file_len
    }

    /// The CRC of the directory's `dirlen` bytes as stored (PAK-4.1).
    #[must_use]
    pub const fn directory_crc(&self) -> u16 {
        self.directory_crc
    }

    /// Whether loading this archive marks the game as modified: its entry count is not
    /// [`PAK_PAK0_COUNT`] or its directory CRC is not [`PAK_PAK0_CRC`].
    ///
    /// Implements PAK-4.2.
    #[must_use]
    pub fn is_modified(&self) -> bool {
        self.entries.len() != PAK_PAK0_COUNT || self.directory_crc != PAK_PAK0_CRC
    }

    /// The first entry, in directory order, whose name is byte-for-byte `name`: the archive
    /// half of id's `COM_FindFile`.
    ///
    /// Implements PAK-3.1.
    #[must_use]
    pub fn find(&self, name: &[u8]) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.name == name)
    }

    /// The data of `entry` in `bytes`, the archive [`Pak::read`] loaded.
    ///
    /// Implements PAK-3.2 to PAK-3.4.
    ///
    /// # Errors
    ///
    /// [`PakError::EntryOutOfBounds`] when the entry's range is not inside the file.
    pub fn entry_data<'a>(&self, bytes: &'a [u8], entry: &Entry) -> Result<&'a [u8], PakError> {
        let range = entry.range(self.file_len)?;
        bytes
            .get(to_usize(range.start)?..to_usize(range.end)?)
            .ok_or(PakError::EntryOutOfBounds {
                filepos: entry.filepos,
                filelen: entry.filelen,
            })
    }

    /// Reads the data of `entry` from `reader`, the archive [`Pak::read_from`] loaded.
    ///
    /// Implements PAK-3.2 to PAK-3.4.
    ///
    /// # Errors
    ///
    /// [`PakError::EntryOutOfBounds`] when the entry's range is not inside the file, and
    /// [`PakError::Io`] when it cannot be read (for example because the file has shrunk).
    pub fn read_entry<R: Read + Seek>(
        &self,
        reader: &mut R,
        entry: &Entry,
    ) -> Result<Vec<u8>, PakError> {
        read_range(reader, entry.range(self.file_len)?)
    }
}

/// Reads the bytes of `range` from `reader`.
fn read_range<R: Read + Seek>(reader: &mut R, range: Range<u64>) -> Result<Vec<u8>, PakError> {
    let len = to_usize(range.end.saturating_sub(range.start))?;
    reader.seek(SeekFrom::Start(range.start))?;
    let mut bytes = vec![0_u8; len];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// The signed little-endian 32-bit integer at `offset`, or 0 past the end of `bytes`.
fn le_i32(bytes: &[u8], offset: usize) -> i32 {
    read_array(bytes, offset).map_or(0, i32::from_le_bytes)
}

/// The `N` bytes at `offset`, or `None` past the end of `bytes`.
fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Option<[u8; N]> {
    bytes.get(offset..offset.checked_add(N)?)?.try_into().ok()
}

/// A memory length as a file length; `usize` is at most 64 bits on every target.
fn len_u64(len: usize) -> u64 {
    u64::try_from(len).unwrap_or(u64::MAX)
}

/// A file offset as a memory index.
fn to_usize(offset: u64) -> Result<usize, PakError> {
    usize::try_from(offset).map_err(|_| PakError::Io(io::ErrorKind::OutOfMemory))
}
