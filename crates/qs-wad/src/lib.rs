// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/wad.c, W_CleanupName, W_LoadWadFile, W_GetLumpinfo, W_GetLumpName, W_GetLumpNum
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/draw.c, Draw_Character

//! Reads WAD2 lump archives: `gfx.wad`, which holds the engine's 2D graphics, and the texture
//! WADs map tools read.
//!
//! [`Wad::read`] is id's `W_LoadWadFile` with the bounds checks id's engine leaves out: it
//! returns a zero-copy view whose lumps borrow the file's bytes, and rejects malformed input
//! with a [`WadError`], never a panic. [`Wad::find`] is `W_GetLumpinfo`, [`Wad::lump`] is
//! `W_GetLumpNum`, and [`QPic`], [`Conchars`] and [`MipTexHeader`] decode the lumps the
//! engine and the map tools read.
//!
//! The format is specified in `docs/spec/wad2.md` (WAD-1 to WAD-4).

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tests;

use core::fmt;

/// The four bytes a WAD2 file starts with (WAD-1.1).
pub const WAD_MAGIC: [u8; 4] = *b"WAD2";

/// Bytes in the header, id's `sizeof(wadinfo_t)` (WAD-1.1).
pub const WAD_HEADER_SIZE: usize = 12;

/// Bytes in one directory entry, id's `sizeof(lumpinfo_t)` (WAD-2.1).
pub const WAD_ENTRY_SIZE: usize = 32;

/// Bytes in a lump name, id's `lumpinfo_t.name` (WAD-2.1, WAD-2.5).
pub const WAD_NAME_SIZE: usize = 16;

/// The WAD2 file the engine loads its 2D graphics from (WAD-1.4).
pub const WAD_GFX_NAME: &str = "gfx.wad";

/// Lump type `TYP_NONE`.
pub const WAD_TYP_NONE: u8 = 0;
/// Lump type `TYP_LABEL`.
pub const WAD_TYP_LABEL: u8 = 1;
/// Lump type `TYP_PALETTE`, also `TYP_LUMPY`.
pub const WAD_TYP_PALETTE: u8 = 64;
/// Lump type `TYP_QTEX`.
pub const WAD_TYP_QTEX: u8 = 65;
/// Lump type `TYP_QPIC`, a [`QPic`] (WAD-4.1).
pub const WAD_TYP_QPIC: u8 = 66;
/// Lump type `TYP_SOUND`.
pub const WAD_TYP_SOUND: u8 = 67;
/// Lump type `TYP_MIPTEX`, a texture starting with a [`MipTexHeader`] (WAD-4.3).
pub const WAD_TYP_MIPTEX: u8 = 68;

/// Compression `CMP_NONE`.
pub const WAD_CMP_NONE: u8 = 0;
/// Compression `CMP_LZSS`; defined by id but never decompressed (WAD-2.7).
pub const WAD_CMP_LZSS: u8 = 1;

/// Width and height in pixels of the `conchars` lump, the row stride in id's
/// `Draw_Character` (WAD-4.2).
pub const WAD_CONCHARS_SIZE: usize = 128;

/// The number of mip levels in a texture, id's `MIPLEVELS` (WAD-4.3).
pub const WAD_MIPLEVELS: usize = 4;

/// Bytes in a [`MipTexHeader`]: a 16-byte name, two sizes and four offsets.
pub const WAD_MIPTEX_HEADER_SIZE: usize = 40;

/// [`WAD_ENTRY_SIZE`] as the type directory sizes are computed in.
const ENTRY_SIZE_U64: u64 = 32;

/// Why a WAD2 file or one of its lumps could not be read.
///
/// Loading errors are written to follow the file's name, as id's are; lookup errors are
/// whole messages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WadError {
    /// The file does not start with `WAD2`, or is shorter than its 12-byte header
    /// (WAD-1.2, WAD-1.3).
    WrongId,
    /// `numlumps` is negative (WAD-2.3).
    NegativeLumpCount {
        /// The stored `numlumps`.
        numlumps: i32,
    },
    /// The directory does not lie wholly inside the file (WAD-2.4).
    DirectoryOutOfBounds {
        /// The stored `infotableofs`.
        infotableofs: i32,
        /// The stored `numlumps`.
        numlumps: i32,
    },
    /// No lump has the requested name (WAD-3.3).
    NotFound {
        /// The name as requested.
        name: Vec<u8>,
    },
    /// A lump index is not below `numlumps` (WAD-3.5).
    IndexOutOfRange {
        /// The requested index.
        index: usize,
    },
    /// A lump's data does not lie wholly inside the file (WAD-3.4).
    LumpOutOfBounds {
        /// The lump's `filepos`.
        filepos: i32,
        /// The lump's `disksize`.
        disksize: i32,
    },
    /// A picture lump's size is negative or its pixels run past the lump (WAD-4.1).
    BadPic {
        /// The stored width.
        width: i32,
        /// The stored height.
        height: i32,
    },
    /// A lump is shorter than its structure (WAD-4.2, WAD-4.3).
    LumpTooShort {
        /// Bytes in the lump.
        len: usize,
        /// Bytes the structure needs.
        needed: usize,
    },
}

impl fmt::Display for WadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongId => f.write_str("doesn't have WAD2 id"),
            Self::NegativeLumpCount { numlumps } => write!(f, "has {numlumps} lumps"),
            Self::DirectoryOutOfBounds {
                infotableofs,
                numlumps,
            } => write!(
                f,
                "has a directory of {numlumps} lumps at offset {infotableofs}, outside the file"
            ),
            Self::NotFound { name } => {
                write!(f, "W_GetLumpinfo: {} not found", name.escape_ascii())
            }
            Self::IndexOutOfRange { index } => write!(f, "W_GetLumpNum: bad number: {index}"),
            Self::LumpOutOfBounds { filepos, disksize } => write!(
                f,
                "lump of {disksize} bytes at offset {filepos} lies outside the file"
            ),
            Self::BadPic { width, height } => write!(
                f,
                "picture of {width} by {height} pixels does not fit its lump"
            ),
            Self::LumpTooShort { len, needed } => {
                write!(
                    f,
                    "lump of {len} bytes is shorter than its {needed}-byte layout"
                )
            }
        }
    }
}

impl core::error::Error for WadError {}

/// A cleaned lump name: id's 16-byte `name` after `W_CleanupName`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LumpName([u8; WAD_NAME_SIZE]);

impl LumpName {
    /// The name's bytes: up to the first NUL, or all 16 when there is none (WAD-2.6).
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        let len = self
            .0
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(WAD_NAME_SIZE);
        self.0.get(..len).unwrap_or_default()
    }
}

/// Cleans a name as id's `W_CleanupName` does.
///
/// Implements WAD-2.5 and WAD-3.2: the bytes of `name` up to the first NUL, and at most 16 of
/// them, are kept with ASCII `A` to `Z` lowered; every other byte is unchanged and the rest
/// is NUL.
#[must_use]
pub fn w_cleanup_name(name: &[u8]) -> LumpName {
    let mut out = [0_u8; WAD_NAME_SIZE];
    for (slot, &byte) in out.iter_mut().zip(name) {
        if byte == 0 {
            break;
        }
        *slot = byte.to_ascii_lowercase();
    }
    LumpName(out)
}

/// One directory entry, id's `lumpinfo_t` after `W_LoadWadFile`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Lump {
    /// Offset of the lump's data from the start of the file.
    pub filepos: i32,
    /// Bytes of the lump as stored.
    pub disksize: i32,
    /// Bytes of the lump uncompressed; never used by the engine.
    pub size: i32,
    /// The lump type, one of the `WAD_TYP_` constants; not checked (WAD-2.7).
    pub lump_type: u8,
    /// The compression, one of the `WAD_CMP_` constants; ignored (WAD-2.7).
    pub compression: u8,
    /// The cleaned name (WAD-2.5).
    pub name: LumpName,
}

/// A loaded WAD2 file: a zero-copy view of its bytes and its cleaned directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wad<'a> {
    bytes: &'a [u8],
    lumps: Vec<Lump>,
}

impl<'a> Wad<'a> {
    /// Loads a WAD2 file held in memory, id's `W_LoadWadFile`.
    ///
    /// Implements WAD-1 (but not WAD-1.4, which is the game filesystem's) and WAD-2: the
    /// directory must lie inside the file, every name is cleaned, and lump ranges are not
    /// checked (WAD-2.8).
    ///
    /// # Errors
    ///
    /// [`WadError::WrongId`], [`WadError::NegativeLumpCount`] or
    /// [`WadError::DirectoryOutOfBounds`].
    pub fn read(bytes: &'a [u8]) -> Result<Self, WadError> {
        let Some(header) = bytes.get(..WAD_HEADER_SIZE) else {
            return Err(WadError::WrongId);
        };
        // One 32-bit comparison, which coverage-guided fuzzers can solve.
        if le_i32(header, 0) != i32::from_le_bytes(WAD_MAGIC) {
            return Err(WadError::WrongId);
        }
        let numlumps = le_i32(header, 4);
        let infotableofs = le_i32(header, 8);
        let Ok(count) = u64::try_from(numlumps) else {
            return Err(WadError::NegativeLumpCount { numlumps });
        };
        let out_of_bounds = WadError::DirectoryOutOfBounds {
            infotableofs,
            numlumps,
        };
        let Ok(start) = u64::try_from(infotableofs) else {
            return Err(out_of_bounds);
        };
        let end = start.saturating_add(count.saturating_mul(ENTRY_SIZE_U64));
        let directory = match (usize::try_from(start), usize::try_from(end)) {
            (Ok(start), Ok(end)) => bytes.get(start..end),
            _ => None,
        };
        let Some(directory) = directory else {
            return Err(out_of_bounds);
        };
        let mut lumps = Vec::new();
        for raw in directory.as_chunks::<WAD_ENTRY_SIZE>().0 {
            lumps.push(Lump {
                filepos: le_i32(raw, 0),
                disksize: le_i32(raw, 4),
                size: le_i32(raw, 8),
                lump_type: raw[12],
                compression: raw[13],
                name: w_cleanup_name(raw.get(16..).unwrap_or_default()),
            });
        }
        Ok(Self { bytes, lumps })
    }

    /// The lumps, in directory order, duplicates included (WAD-2.9).
    #[must_use]
    pub fn lumps(&self) -> &[Lump] {
        &self.lumps
    }

    /// The first lump, in directory order, whose cleaned name equals `name` cleaned, id's
    /// `W_GetLumpinfo`.
    ///
    /// Implements WAD-3.1 to WAD-3.3.
    ///
    /// # Errors
    ///
    /// [`WadError::NotFound`] when no lump has the name.
    pub fn find(&self, name: &[u8]) -> Result<&Lump, WadError> {
        let clean = w_cleanup_name(name);
        self.lumps
            .iter()
            .find(|lump| lump.name == clean)
            .ok_or_else(|| WadError::NotFound {
                name: name.to_vec(),
            })
    }

    /// The lump at `index`, id's `W_GetLumpNum`.
    ///
    /// Implements WAD-3.5: `index` must be below `numlumps`.
    ///
    /// # Errors
    ///
    /// [`WadError::IndexOutOfRange`] otherwise.
    pub fn lump(&self, index: usize) -> Result<&Lump, WadError> {
        self.lumps
            .get(index)
            .ok_or(WadError::IndexOutOfRange { index })
    }

    /// The `disksize` bytes of `lump` at its `filepos`, as stored (WAD-2.7, WAD-3.4).
    ///
    /// # Errors
    ///
    /// [`WadError::LumpOutOfBounds`] when the range is not inside the file.
    pub fn data(&self, lump: &Lump) -> Result<&'a [u8], WadError> {
        let range = match (
            usize::try_from(lump.filepos),
            usize::try_from(lump.disksize),
        ) {
            (Ok(start), Ok(len)) => start.checked_add(len).map(|end| start..end),
            _ => None,
        };
        range
            .and_then(|range| self.bytes.get(range))
            .ok_or(WadError::LumpOutOfBounds {
                filepos: lump.filepos,
                disksize: lump.disksize,
            })
    }
}

/// A picture lump, id's `qpic_t`: a size and its palette indices (WAD-4.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QPic<'a> {
    /// Width in pixels.
    pub width: usize,
    /// Height in pixels.
    pub height: usize,
    /// `width * height` palette indices, row by row from the top left.
    pub pixels: &'a [u8],
}

impl<'a> QPic<'a> {
    /// Decodes a picture lump.
    ///
    /// Implements WAD-4.1: `width` and `height` are signed little-endian 32-bit integers,
    /// then `width * height` bytes. Bytes after the pixels are ignored.
    ///
    /// # Errors
    ///
    /// [`WadError::LumpTooShort`] when the lump has no 8-byte size, and [`WadError::BadPic`]
    /// when a dimension is negative or the pixels run past the lump.
    pub fn parse(data: &'a [u8]) -> Result<Self, WadError> {
        if data.len() < 8 {
            return Err(WadError::LumpTooShort {
                len: data.len(),
                needed: 8,
            });
        }
        let (width, height) = (le_i32(data, 0), le_i32(data, 4));
        let bad = WadError::BadPic { width, height };
        let (Ok(w), Ok(h)) = (usize::try_from(width), usize::try_from(height)) else {
            return Err(bad);
        };
        let pixels = w
            .checked_mul(h)
            .and_then(|len| len.checked_add(8))
            .and_then(|end| data.get(8..end))
            .ok_or(bad)?;
        Ok(Self {
            width: w,
            height: h,
            pixels,
        })
    }
}

/// The `conchars` lump: 128 by 128 palette indices holding 256 characters of 8 by 8 pixels
/// (WAD-4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Conchars<'a> {
    pixels: &'a [u8],
}

/// The palette index `Draw_Character` does not draw (WAD-4.2).
pub const WAD_CONCHARS_TRANSPARENT: u8 = 0;

impl<'a> Conchars<'a> {
    /// The bytes the `conchars` lump needs.
    const LEN: usize = WAD_CONCHARS_SIZE.saturating_mul(WAD_CONCHARS_SIZE);

    /// Takes the first 128 by 128 bytes of the `conchars` lump, which has no header.
    ///
    /// # Errors
    ///
    /// [`WadError::LumpTooShort`] when the lump has fewer than 16384 bytes.
    pub fn parse(data: &'a [u8]) -> Result<Self, WadError> {
        data.get(..Self::LEN)
            .map(|pixels| Self { pixels })
            .ok_or(WadError::LumpTooShort {
                len: data.len(),
                needed: Self::LEN,
            })
    }

    /// The 8 by 8 pixels of character `n`, row by row, as id's `Draw_Character` reads them:
    /// row `n >> 4` and column `n & 15` of the 16 by 16 grid.
    ///
    /// Implements WAD-4.2.
    #[must_use]
    pub fn glyph(&self, n: u8) -> [u8; 64] {
        let row = usize::from(n >> 4);
        let column = usize::from(n & 15);
        let mut glyph = [0_u8; 64];
        for (y, out) in glyph.as_chunks_mut::<8>().0.iter_mut().enumerate() {
            let start = row
                .saturating_mul(8)
                .saturating_add(y)
                .saturating_mul(WAD_CONCHARS_SIZE)
                .saturating_add(column.saturating_mul(8));
            if let Some(line) = self.pixels.get(start..start.saturating_add(8)) {
                out.copy_from_slice(line);
            }
        }
        glyph
    }
}

/// The start of a texture lump, id's `miptex_t` (WAD-4.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MipTexHeader {
    /// The texture's name, 16 bytes as stored.
    pub name: [u8; WAD_NAME_SIZE],
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Offset of each mip level from the start of the lump.
    pub offsets: [u32; WAD_MIPLEVELS],
}

impl MipTexHeader {
    /// Decodes the 40-byte header at the start of a texture lump.
    ///
    /// Implements WAD-4.3: a 16-byte name, then `width`, `height` and four mip offsets as
    /// unsigned little-endian 32-bit integers. The offsets are not checked.
    ///
    /// # Errors
    ///
    /// [`WadError::LumpTooShort`] when the lump is shorter than 40 bytes.
    pub fn parse(data: &[u8]) -> Result<Self, WadError> {
        let Some(header) = data.get(..WAD_MIPTEX_HEADER_SIZE) else {
            return Err(WadError::LumpTooShort {
                len: data.len(),
                needed: WAD_MIPTEX_HEADER_SIZE,
            });
        };
        let mut name = [0_u8; WAD_NAME_SIZE];
        name.copy_from_slice(header.get(..WAD_NAME_SIZE).unwrap_or(&[0; WAD_NAME_SIZE]));
        let word = |index: usize| {
            le_i32(
                header,
                WAD_NAME_SIZE.saturating_add(index.saturating_mul(4)),
            )
            .cast_unsigned()
        };
        Ok(Self {
            name,
            width: word(0),
            height: word(1),
            offsets: [word(2), word(3), word(4), word(5)],
        })
    }
}

/// The signed little-endian 32-bit integer at `offset`, or 0 past the end of `bytes`.
fn le_i32(bytes: &[u8], offset: usize) -> i32 {
    read_array(bytes, offset).map_or(0, i32::from_le_bytes)
}

/// The `N` bytes at `offset`, or `None` past the end of `bytes`.
fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Option<[u8; N]> {
    bytes.get(offset..offset.checked_add(N)?)?.try_into().ok()
}
