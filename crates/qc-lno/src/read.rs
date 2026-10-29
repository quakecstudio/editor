// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading a `.lno` file into an [`LnoFile`].
//!
//! Implements LNO-5 of `docs/spec/lno.md`: the checks run in the order LNO-5.6 and the first
//! failure is reported, reading never panics for any input, and a file is read with its line
//! table, optional column table and trailing bytes so the writer can reproduce it (LNO-6.2).

use crate::{LNO_HEADER_SIZE, LNO_MAGIC, LNO_VERSION, LnoFile};

/// Why a `.lno` file could not be read. Each variant is one of the LNO-5 checks, in the order
/// they run (LNO-5.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// The file is shorter than the 24-byte header (LNO-5.1): its actual length.
    TooShort(usize),
    /// The first four bytes are not exactly `LNOF` (LNO-5.2): the four bytes found.
    BadMagic([u8; 4]),
    /// The version is not 1 (LNO-5.3): the version found.
    UnsupportedVersion(u32),
    /// The line table is truncated (LNO-5.4): `numstatements` and the file's actual length.
    LineTableTruncated {
        /// The header's `numstatements`.
        numstatements: u32,
        /// The file's actual length.
        len: usize,
    },
}

impl core::fmt::Display for ReadError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TooShort(len) => write!(
                f,
                "the .lno is {len} bytes, shorter than the {LNO_HEADER_SIZE}-byte header"
            ),
            Self::BadMagic(bytes) => {
                write!(f, "the .lno does not start with LNOF: found {bytes:?}")
            }
            Self::UnsupportedVersion(version) => {
                write!(f, "the .lno has version {version}, not {LNO_VERSION}")
            }
            Self::LineTableTruncated { numstatements, len } => write!(
                f,
                "the .lno is {len} bytes: too short for a {numstatements}-statement line table"
            ),
        }
    }
}

impl core::error::Error for ReadError {}

/// Reads the little-endian `u32` at byte `offset` of `bytes`. The caller has already checked
/// that `offset + 4` is within the slice, so the `get` never returns `None`; this returns 0 if
/// it ever did, which no caller relies on.
fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    read_array(bytes, offset).map_or(0, u32::from_le_bytes)
}

/// The `N` bytes at `offset`, or `None` past the end of `bytes`.
fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Option<[u8; N]> {
    bytes.get(offset..offset.checked_add(N)?)?.try_into().ok()
}

impl LnoFile {
    /// Reads a `.lno` file from `bytes` (LNO-5).
    ///
    /// The checks run in order (LNO-5.6): length (LNO-5.1), magic (LNO-5.2), version
    /// (LNO-5.3), then the line table length and whether a column table follows (LNO-5.4).
    /// Line and column values and the three counts other than `numstatements` are kept exactly
    /// as read (LNO-3.5, LNO-5.7). Bytes after the last table are kept as trailing bytes
    /// (LNO-5.5). Reading does not need the `progs.dat`; use [`LnoFile::matches`] for the
    /// LNO-4.1 check (LNO-5.6).
    ///
    /// # Errors
    ///
    /// Returns a [`ReadError`] for a file shorter than the header, a bad magic, an unsupported
    /// version, or a truncated line table. It never panics for any input.
    pub fn read(bytes: &[u8]) -> Result<Self, ReadError> {
        // LNO-5.1: a file shorter than 24 bytes is rejected.
        if bytes.len() < LNO_HEADER_SIZE {
            return Err(ReadError::TooShort(bytes.len()));
        }
        // LNO-5.2: the first four bytes must be exactly `LNOF`.
        let magic = <[u8; 4]>::try_from(bytes.get(0..4).unwrap_or(&[])).unwrap_or([0; 4]);
        if magic != LNO_MAGIC {
            return Err(ReadError::BadMagic(magic));
        }
        // LNO-5.3: a version other than 1 is rejected, carrying the version.
        let version = read_u32(bytes, 4);
        if version != LNO_VERSION {
            return Err(ReadError::UnsupportedVersion(version));
        }
        let numglobaldefs = read_u32(bytes, 8);
        let numglobals = read_u32(bytes, 12);
        let numfielddefs = read_u32(bytes, 16);
        let numstatements = read_u32(bytes, 20);

        // LNO-5.4: with n the header's numstatements, computed without overflow, a file
        // shorter than 24 + 4n bytes has a truncated line table.
        let n = usize::try_from(numstatements).unwrap_or(usize::MAX);
        let line_bytes = n.checked_mul(4);
        let line_end = line_bytes.and_then(|b| b.checked_add(LNO_HEADER_SIZE));
        let Some(line_end) = line_end else {
            return Err(ReadError::LineTableTruncated {
                numstatements,
                len: bytes.len(),
            });
        };
        if bytes.len() < line_end {
            return Err(ReadError::LineTableTruncated {
                numstatements,
                len: bytes.len(),
            });
        }

        // LNO-3.1: the line table is numstatements i32 values after the header.
        let lines = read_i32_table(bytes, LNO_HEADER_SIZE, n);

        // LNO-5.4: a file of at least 24 + 8n bytes, with n above 0, has a column table.
        let col_end = line_bytes
            .and_then(|b| b.checked_mul(2))
            .and_then(|b| b.checked_add(LNO_HEADER_SIZE));
        let has_columns = n > 0 && col_end.is_some_and(|end| bytes.len() >= end);
        let (columns, tables_end) = if has_columns {
            // LNO-3.3: the column table is numstatements i32 values after the line table.
            let cols = read_i32_table(bytes, line_end, n);
            let end = col_end.unwrap_or(line_end);
            (Some(cols), end)
        } else {
            (None, line_end)
        };

        // LNO-5.5: bytes after the last table are kept as trailing bytes.
        let trailing = bytes.get(tables_end..).unwrap_or_default().to_vec();

        Ok(Self {
            numglobaldefs,
            numglobals,
            numfielddefs,
            lines,
            columns,
            trailing,
        })
    }
}

/// Reads `count` little-endian `i32` values from `bytes` starting at `offset`. The caller has
/// checked the slice is long enough; a value that cannot be read (never, given that check) is
/// read as 0.
fn read_i32_table(bytes: &[u8], offset: usize, count: usize) -> Vec<i32> {
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        let start = index
            .checked_mul(4)
            .and_then(|delta| offset.checked_add(delta));
        let value = start
            .and_then(|start| read_array(bytes, start))
            .map_or(0, i32::from_le_bytes);
        out.push(value);
    }
    out
}
