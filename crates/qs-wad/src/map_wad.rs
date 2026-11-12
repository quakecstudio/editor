// SPDX-License-Identifier: GPL-3.0-or-later

//! The texture WADs a map names, as QuakeSpasm reads them (`docs/spec/wad3.md`, P5-11 and
//! P5-12): WAD2 and Half-Life's WAD3, described from QuakeSpasm's `W_AddWadFile` and
//! `Mod_LoadWadTexture`, not ported. A WAD3 texture carries its own palette after its pixels.
//!
//! [`MapWad::read`] reads a file's header and directory as `W_AddWadFile` does, keeping its
//! sanity fixes of the directory; [`MapWad::texture`] finds and reads a texture as
//! `Mod_LoadWadTexture` does, file-bounded where QuakeSpasm's reads are. `gfx.wad` is read by
//! [`crate::Wad`], id's `W_LoadWadFile`, which takes WAD2 only (WAD-1.2).

use core::fmt;

use crate::{
    ENTRY_COMPRESSION, ENTRY_DISKSIZE, ENTRY_NAME, ENTRY_SIZE, ENTRY_TYPE, HEADER_INFOTABLEOFS,
    HEADER_NUMLUMPS, LumpName, WAD_ENTRY_SIZE, WAD_HEADER_SIZE, WAD_MAGIC, WAD_MIPTEX_HEADER_SIZE,
    WAD_NAME_SIZE, le_i32, w_cleanup_name,
};

/// The four bytes a WAD3 file starts with, QuakeSpasm's `WADID_VALVE` (WAD3-1.1).
pub const WAD3_MAGIC: [u8; 4] = *b"WAD3";

/// A texture's width and height must be multiples of 16: `Texture %s (%d x %d) is not 16
/// aligned` (WAD3-3).
const ALIGN_MASK: u32 = 15;
/// The bytes of one colour of a WAD3 palette, `colors * 3` (WAD3-3.3).
const COLOR_BYTES: i64 = 3;

/// A Half-Life texture with its own palette, QuakeSpasm's `TYP_MIPTEX_PALETTE` (WAD3-3.2).
pub const WAD_TYP_MIPTEX_PALETTE: u8 = 67;

/// A WAD's identification, of the two `W_AddWadFile` takes (WAD3-1.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WadKind {
    /// `WAD2`, Quake's.
    Wad2,
    /// `WAD3`, Half-Life's.
    Wad3,
}

/// Why a map WAD is not used (WAD3-1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapWadError {
    /// The file is shorter than its 12-byte header, or does not start `WAD2` or `WAD3`:
    /// QuakeSpasm's `%s is not a valid WAD`.
    NotValid,
    /// A negative lump count or directory offset: `%s is not a valid WAD (%i lumps, %i info
    /// table offset)`.
    BadDirectory {
        /// The lump count.
        numlumps: i32,
        /// The directory's offset.
        infotableofs: i32,
    },
    /// No lumps: `WAD file %s has no lumps, ignored`.
    NoLumps,
}

impl fmt::Display for MapWadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotValid => f.write_str("not a valid WAD"),
            Self::BadDirectory {
                numlumps,
                infotableofs,
            } => write!(
                f,
                "not a valid WAD ({numlumps} lumps, {infotableofs} info table offset)"
            ),
            Self::NoLumps => f.write_str("the WAD has no lumps"),
        }
    }
}

impl core::error::Error for MapWadError {}

/// One directory entry after `W_AddWadFile`'s sanity fixes (WAD3-2.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapWadEntry {
    /// The cleaned name (WAD3-2.2).
    pub name: LumpName,
    /// The lump's offset in the file, after the fixes.
    pub filepos: i64,
    /// The lump's size, after the fixes.
    pub size: i64,
    /// The lump's type.
    pub ty: u8,
    /// The compression byte, which nothing reads.
    pub compression: u8,
}

/// A texture read from a map WAD (WAD3-3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WadTexture<'a> {
    /// The name in the texture's own header, up to its first NUL.
    pub name: Vec<u8>,
    /// Its width.
    pub width: u32,
    /// Its height.
    pub height: u32,
    /// Mip level 0's bytes, as many as the lump and the file give (WAD3-3.4).
    pub pixels: &'a [u8],
    /// For a WAD3 palette texture, the bytes after the four mip levels: the 16-bit colour
    /// count and that many RGB triples, cut as WAD3-3.3 and WAD3-3.4 say; `None` otherwise.
    pub palette: Option<&'a [u8]>,
}

/// A WAD a map names, WAD2 or WAD3 (WAD3-1, WAD3-2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapWad<'a> {
    bytes: &'a [u8],
    kind: WadKind,
    entries: Vec<MapWadEntry>,
}

impl<'a> MapWad<'a> {
    /// Reads a WAD's header and directory as QuakeSpasm's `W_AddWadFile`.
    ///
    /// Implements WAD3-1 and WAD3-2.
    ///
    /// # Errors
    ///
    /// [`MapWadError`] for a file QuakeSpasm does not use.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "spec WAD3-2.3: 32-bit fields widened to 64 bits, whose sums cannot overflow"
    )]
    pub fn read(bytes: &'a [u8]) -> Result<Self, MapWadError> {
        let header = bytes.get(..WAD_HEADER_SIZE).ok_or(MapWadError::NotValid)?;
        let kind = match header.get(..WAD_MAGIC.len()) {
            Some(id) if id == WAD_MAGIC => WadKind::Wad2,
            Some(id) if id == WAD3_MAGIC => WadKind::Wad3,
            _ => return Err(MapWadError::NotValid),
        };
        let numlumps = le_i32(header, HEADER_NUMLUMPS);
        let infotableofs = le_i32(header, HEADER_INFOTABLEOFS);
        if numlumps < 0 || infotableofs < 0 {
            return Err(MapWadError::BadDirectory {
                numlumps,
                infotableofs,
            });
        }
        if numlumps == 0 {
            return Err(MapWadError::NoLumps);
        }
        let length = len_i64(bytes);
        let mut entries = Vec::new();
        // WAD3-2.4: only the entries the file holds whole are entries.
        let start = usize::try_from(infotableofs).unwrap_or(usize::MAX);
        let table = bytes.get(start..).unwrap_or_default();
        for raw in table
            .as_chunks::<WAD_ENTRY_SIZE>()
            .0
            .iter()
            .take(usize::try_from(numlumps).unwrap_or(0))
        {
            let mut filepos = i64::from(le_i32(raw, 0));
            let disksize = i64::from(le_i32(raw, ENTRY_DISKSIZE));
            let mut size = i64::from(le_i32(raw, ENTRY_SIZE));
            // WAD3-2.3: `W_AddWadFile`'s fixes, in its order.
            if filepos + size > length && filepos + disksize <= length {
                size = disksize;
            }
            if filepos < 0 || size < 0 || filepos + size > length {
                if filepos > length || size < 0 {
                    filepos = 0;
                    size = 0.max(size - filepos);
                } else {
                    size = 0.max(size - filepos);
                }
            }
            entries.push(MapWadEntry {
                name: w_cleanup_name(
                    raw.get(ENTRY_NAME..ENTRY_NAME.saturating_add(WAD_NAME_SIZE))
                        .unwrap_or_default(),
                ),
                filepos,
                size,
                ty: raw.get(ENTRY_TYPE).copied().unwrap_or(0),
                compression: raw.get(ENTRY_COMPRESSION).copied().unwrap_or(0),
            });
        }
        Ok(Self {
            bytes,
            kind,
            entries,
        })
    }

    /// Which identification the file has.
    #[must_use]
    pub const fn kind(&self) -> WadKind {
        self.kind
    }

    /// The directory, after `W_AddWadFile`'s fixes.
    #[must_use]
    pub fn entries(&self) -> &[MapWadEntry] {
        &self.entries
    }

    /// The first entry whose cleaned name is `name`'s, `W_GetLumpinfo` (WAD3-3.1).
    #[must_use]
    pub fn find(&self, name: &[u8]) -> Option<&MapWadEntry> {
        let clean = w_cleanup_name(name);
        self.entries.iter().find(|entry| entry.name == clean)
    }

    /// The texture named `name`, as QuakeSpasm's `Mod_LoadWadTexture` reads it; `None` where
    /// QuakeSpasm falls back to the BSP's own (WAD3-3).
    #[must_use]
    #[expect(
        clippy::arithmetic_side_effects,
        clippy::integer_division,
        reason = "spec WAD3-3.3, WAD3-3.4: QuakeSpasm's int sums of fields, widened to 64 bits so \
                  none overflows, and its `pixels / 64 * 85`"
    )]
    pub fn texture(&self, name: &[u8]) -> Option<WadTexture<'a>> {
        let entry = self.find(name)?;
        // WAD3-3.2: a miptex, or in a WAD3 a palette miptex.
        let pal = self.kind == WadKind::Wad3 && entry.ty == WAD_TYP_MIPTEX_PALETTE;
        if entry.ty != crate::WAD_TYP_MIPTEX && !pal {
            return None;
        }
        // WAD3-3.5: a lump that starts before the file is no texture.
        let filepos = usize::try_from(entry.filepos).ok()?;
        let header = self
            .bytes
            .get(filepos..filepos.checked_add(WAD_MIPTEX_HEADER_SIZE)?)
            .and_then(|header| crate::MipTexHeader::parse(header).ok())?;
        if header.width == 0 || header.height == 0 {
            return None;
        }
        if header.width & ALIGN_MASK != 0 || header.height & ALIGN_MASK != 0 {
            return None;
        }
        // `pixels = mt.width * mt.height` in an int: a product that does not fit is none.
        let pixels = i64::from(header.width.wrapping_mul(header.height));
        if i32::try_from(pixels).is_err() {
            return None;
        }
        let mips = i64::try_from(crate::WAD_MIP_PIXELS)
            .ok()
            .zip(i64::try_from(crate::WAD_MIP_BYTES).ok())
            .map(|(per, bytes)| pixels / per * bytes)?;
        let pixels_pos = entry.filepos + i64::try_from(WAD_MIPTEX_HEADER_SIZE).ok()?;
        let (mut palette, lump_bytes) = if pal {
            // WAD3-3.3: the count when `pixels / 64 * 85 + 2 <= info->size`.
            let mut palette = 2_i64;
            if mips + 2 <= entry.size {
                palette += i64::from(self.u16_at(pixels_pos + mips)) * COLOR_BYTES;
            }
            (palette, mips + palette)
        } else {
            (0, pixels)
        };
        let mut pixels = pixels;
        if lump_bytes > entry.size {
            // WAD3-3.4: cut against the lump's size, counted from its start.
            let lump_bytes = entry.size;
            if pal {
                palette = palette.min(0.max(lump_bytes - mips));
            }
            pixels = pixels.min(lump_bytes);
        }
        let name_len = header
            .name
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(header.name.len());
        Some(WadTexture {
            name: header.name.get(..name_len).unwrap_or_default().to_vec(),
            width: header.width,
            height: header.height,
            pixels: self.read_at(pixels_pos, pixels),
            palette: pal.then(|| self.read_at(pixels_pos + mips, palette)),
        })
    }

    /// `count` bytes from `at`, as many as the file holds: `FS_fseek` stops at the end and
    /// `FS_fread` reads to it.
    fn read_at(&self, at: i64, count: i64) -> &'a [u8] {
        let length = len_i64(self.bytes);
        let start = at.clamp(0, length);
        let end = start.saturating_add(count.max(0)).min(length);
        let (Ok(start), Ok(end)) = (usize::try_from(start), usize::try_from(end)) else {
            return &[];
        };
        self.bytes.get(start..end).unwrap_or_default()
    }

    /// The little-endian count at `at`; 0 when the file does not hold both bytes (WAD3-3.3).
    fn u16_at(&self, at: i64) -> u16 {
        match self.read_at(at, 2) {
            [low, high] => u16::from_le_bytes([*low, *high]),
            _ => 0,
        }
    }
}

/// A file's length as QuakeSpasm's `long` arithmetic takes it.
fn len_i64(bytes: &[u8]) -> i64 {
    i64::try_from(bytes.len()).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests;
