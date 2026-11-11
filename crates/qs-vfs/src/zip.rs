// SPDX-License-Identifier: GPL-3.0-or-later

//! A zip archive's members, as the rerelease's `QuakeEX.kpf` holds its localization (FS-5.3,
//! FS-5.5): the central directory read once, then a member by name, stored or deflated (PKWARE
//! APPNOTE). Inflating is `miniz_oxide`'s; the container is read here. Every malformed input is
//! an error, never a panic.

use std::fmt;
use std::io::{self, Read, Seek, SeekFrom};

use miniz_oxide::inflate::{TINFLStatus, decompress_to_vec_with_limit};

/// Why a zip, or a member of it, could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ZipError {
    /// Reading the source failed: the error's kind and the system's message.
    Io {
        /// The kind of error.
        kind: io::ErrorKind,
        /// The system's message, such as "No such file or directory (os error 2)".
        message: String,
    },
    /// No end-of-central-directory record in the last 64 KiB.
    NotZip,
    /// A zip64 archive or member.
    Zip64,
    /// The central directory lies past the end of the file.
    CentralOutside,
    /// The central directory ends before its last record.
    CentralTruncated,
    /// The archive spans more than one disk.
    Multidisk,
    /// The member is encrypted, plainly or strongly.
    Encrypted,
    /// The member holds compressed patch data.
    Patch,
    /// The member's bytes do not have its stated CRC-32.
    Crc,
    /// The member's local header is not one.
    LocalHeader,
    /// The member's data ends before its stated compressed size.
    Truncated,
    /// The member's stated size does not fit in memory.
    TooLarge,
    /// The member inflates to more than its stated size.
    Larger,
    /// The member's deflate stream is corrupt.
    Corrupt,
    /// The member uses a method other than stored (0) or deflated (8).
    Method(u16),
    /// The member is not its stated size.
    Size,
}

impl fmt::Display for ZipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { message, .. } => f.write_str(message),
            Self::NotZip => f.write_str("not a zip file: no end of central directory"),
            Self::Zip64 => f.write_str("zip64 archives and members are not supported"),
            Self::CentralOutside => f.write_str("zip central directory lies outside the file"),
            Self::CentralTruncated => f.write_str("truncated zip central directory"),
            Self::Multidisk => f.write_str("multi-disk zip archives are not supported"),
            Self::Encrypted => f.write_str("encrypted zip members are not supported"),
            Self::Patch => f.write_str("zip members of patch data are not supported"),
            Self::Crc => f.write_str("zip member does not match its CRC-32"),
            Self::LocalHeader => f.write_str("bad zip local header"),
            Self::Truncated => f.write_str("truncated zip member"),
            Self::TooLarge => f.write_str("zip member too large"),
            Self::Larger => f.write_str("zip member is larger than its stated size"),
            Self::Corrupt => f.write_str("corrupt deflate data in zip member"),
            Self::Method(method) => {
                write!(
                    f,
                    "zip member uses an unsupported compression method ({method})"
                )
            }
            Self::Size => f.write_str("zip member is not its stated size"),
        }
    }
}

impl core::error::Error for ZipError {}

impl From<io::Error> for ZipError {
    fn from(error: io::Error) -> Self {
        Self::Io {
            kind: error.kind(),
            message: error.to_string(),
        }
    }
}

/// One member of a zip's central directory.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Member {
    name: String,
    method: u16,
    flags: u16,
    compressed: u64,
    size: u64,
    offset: u64,
    crc: u32,
}

/// A zip archive: its central directory, read once, and the source to read members from.
#[derive(Debug)]
pub struct Zip<R> {
    source: R,
    members: Vec<Member>,
}

/// The end-of-central-directory record's signature.
const EOCD: u32 = 0x0605_4b50;
/// A central directory record's signature.
const CENTRAL: u32 = 0x0201_4b50;
/// A local header's signature.
const LOCAL: u32 = 0x0403_4b50;
/// The end-of-central-directory record, without its comment.
const EOCD_SIZE: usize = 22;
/// The longest archive comment.
const MAX_COMMENT: usize = 0xffff;
/// A local header, without its name and extra field.
const LOCAL_SIZE: usize = 30;
/// The general-purpose flags miniz refuses: encryption (bit 0), patch data (5) and strong
/// encryption (6).
const ENCRYPTED: u16 = 1;
const PATCH: u16 = 1 << 5;
const STRONG: u16 = 1 << 6;

impl<R: Read + Seek> Zip<R> {
    /// Reads the central directory of the zip `source` holds.
    ///
    /// # Errors
    ///
    /// [`ZipError`] when the source cannot be read or holds no zip this reads.
    pub fn open(mut source: R) -> Result<Self, ZipError> {
        let len = source.seek(SeekFrom::End(0))?;
        if len < u64::try_from(EOCD_SIZE).unwrap_or(u64::MAX) {
            return Err(ZipError::NotZip);
        }
        let tail_len = len.min(u64::try_from(EOCD_SIZE.saturating_add(MAX_COMMENT)).unwrap_or(len));
        source.seek(SeekFrom::Start(len.saturating_sub(tail_len)))?;
        let mut tail = Vec::new();
        (&mut source).take(tail_len).read_to_end(&mut tail)?;
        let at = (0..=tail.len().saturating_sub(EOCD_SIZE))
            .rev()
            .find(|&i| u32_at(&tail, i) == Some(EOCD))
            .ok_or(ZipError::NotZip)?;
        let this_disk = u16_at(&tail, at.saturating_add(4)).unwrap_or_default();
        let cd_disk = u16_at(&tail, at.saturating_add(6)).unwrap_or_default();
        let on_disk = u16_at(&tail, at.saturating_add(8)).unwrap_or_default();
        let count = u16_at(&tail, at.saturating_add(10)).unwrap_or_default();
        // miniz: a disk number other than 0 is refused unless both are 1, and every entry
        // must be on this disk.
        if ((this_disk | cd_disk) != 0 && (this_disk != 1 || cd_disk != 1)) || on_disk != count {
            return Err(ZipError::Multidisk);
        }
        let cd_size = u32_at(&tail, at.saturating_add(12)).unwrap_or_default();
        let cd_offset = u32_at(&tail, at.saturating_add(16)).unwrap_or_default();
        if count == 0xffff || cd_size == u32::MAX || cd_offset == u32::MAX {
            return Err(ZipError::Zip64);
        }
        if u64::from(cd_offset).saturating_add(u64::from(cd_size)) > len {
            return Err(ZipError::CentralOutside);
        }
        source.seek(SeekFrom::Start(u64::from(cd_offset)))?;
        let mut cd = Vec::new();
        (&mut source)
            .take(u64::from(cd_size))
            .read_to_end(&mut cd)?;
        let mut members = Vec::new();
        let mut at = 0_usize;
        for _ in 0..count {
            let bad = || ZipError::CentralTruncated;
            if u32_at(&cd, at) != Some(CENTRAL) {
                return Err(bad());
            }
            let flags = u16_at(&cd, at.saturating_add(8)).ok_or_else(bad)?;
            let method = u16_at(&cd, at.saturating_add(10)).ok_or_else(bad)?;
            let crc = u32_at(&cd, at.saturating_add(16)).ok_or_else(bad)?;
            let compressed = u32_at(&cd, at.saturating_add(20)).ok_or_else(bad)?;
            let size = u32_at(&cd, at.saturating_add(24)).ok_or_else(bad)?;
            let name_len = usize::from(u16_at(&cd, at.saturating_add(28)).ok_or_else(bad)?);
            let extra_len = usize::from(u16_at(&cd, at.saturating_add(30)).ok_or_else(bad)?);
            let comment_len = usize::from(u16_at(&cd, at.saturating_add(32)).ok_or_else(bad)?);
            let offset = u32_at(&cd, at.saturating_add(42)).ok_or_else(bad)?;
            let name_at = at.saturating_add(46);
            let name = cd
                .get(name_at..name_at.saturating_add(name_len))
                .ok_or_else(bad)?;
            if [compressed, size, offset].contains(&u32::MAX) {
                return Err(ZipError::Zip64);
            }
            members.push(Member {
                name: String::from_utf8_lossy(name).into_owned(),
                method,
                flags,
                compressed: u64::from(compressed),
                size: u64::from(size),
                offset: u64::from(offset),
                crc,
            });
            at = name_at
                .saturating_add(name_len)
                .saturating_add(extra_len)
                .saturating_add(comment_len);
        }
        Ok(Self { source, members })
    }

    /// The bytes of the member called `name`, or `None` when the archive has no such member:
    /// one of exactly that name, else the first whose name matches it ignoring ASCII case, as
    /// miniz finds a member without `MZ_ZIP_FLAG_CASE_SENSITIVE`.
    ///
    /// # Errors
    ///
    /// [`ZipError`] when the member cannot be read or inflated, is not its stated size or CRC,
    /// or is encrypted or patch data.
    pub fn read(&mut self, name: &str) -> Result<Option<Vec<u8>>, ZipError> {
        let found = self.members.iter().find(|m| m.name == name).or_else(|| {
            self.members
                .iter()
                .find(|m| m.name.eq_ignore_ascii_case(name))
        });
        let Some(member) = found else {
            return Ok(None);
        };
        // "A directory or zero length file": miniz returns it, empty, before any other check.
        if member.compressed == 0 {
            return Ok(Some(Vec::new()));
        }
        if member.flags & ENCRYPTED != 0 || member.flags & STRONG != 0 {
            return Err(ZipError::Encrypted);
        }
        if member.flags & PATCH != 0 {
            return Err(ZipError::Patch);
        }
        self.source.seek(SeekFrom::Start(member.offset))?;
        let mut local = [0_u8; LOCAL_SIZE];
        self.source.read_exact(&mut local)?;
        if u32_at(&local, 0) != Some(LOCAL) {
            return Err(ZipError::LocalHeader);
        }
        let skip = u16_at(&local, 26)
            .zip(u16_at(&local, 28))
            .map(|(n, e)| u64::from(n).saturating_add(u64::from(e)))
            .unwrap_or_default();
        self.source
            .seek(SeekFrom::Current(i64::try_from(skip).unwrap_or_default()))?;
        let mut data = Vec::new();
        (&mut self.source)
            .take(member.compressed)
            .read_to_end(&mut data)?;
        if u64::try_from(data.len()).ok() != Some(member.compressed) {
            return Err(ZipError::Truncated);
        }
        let size = usize::try_from(member.size).map_err(|_| ZipError::TooLarge)?;
        let bytes = match member.method {
            0 => data,
            8 => decompress_to_vec_with_limit(&data, size).map_err(|e| match e.status {
                TINFLStatus::HasMoreOutput => ZipError::Larger,
                _ => ZipError::Corrupt,
            })?,
            other => return Err(ZipError::Method(other)),
        };
        if bytes.len() != size {
            return Err(ZipError::Size);
        }
        if crc32(&bytes) != member.crc {
            return Err(ZipError::Crc);
        }
        Ok(Some(bytes))
    }
}

/// The CRC-32 of zip (ISO 3309, the reflected polynomial `0xEDB88320`), which a member's
/// central directory record states of its uncompressed bytes.
#[must_use]
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            // All ones when the low bit is set, else zero.
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// The little-endian `u16` at `at`.
fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    let b: [u8; 2] = bytes.get(at..at.checked_add(2)?)?.try_into().ok()?;
    Some(u16::from_le_bytes(b))
}

/// The little-endian `u32` at `at`.
fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let b: [u8; 4] = bytes.get(at..at.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(b))
}

mod write;

pub use write::{NewMember, ZipWriteError, write_zip};

#[cfg(test)]
mod tests;
