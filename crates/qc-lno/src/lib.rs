// SPDX-License-Identifier: GPL-3.0-or-later

//! Reads and writes the `.lno` line map that a QuakeC compiler writes beside a `progs.dat`.
//!
//! A `.lno` gives the source line, and optionally the source column, of every statement in a
//! `progs.dat`. id never defined it; the format is described in `docs/spec/lno.md` from the
//! programs that write and read it (FTEQCC and FTE, GMQCC, DarkPlaces). No file here is ported
//! from a registered upstream.
//!
//! [`LnoFile::read`] checks a file in the order of LNO-5.6 and returns an [`LnoFile`], or a
//! typed [`ReadError`], never a panic, for any input. [`LnoFile::write`] turns an [`LnoFile`]
//! back into bytes; a file the reader accepts is written back byte for byte (LNO-6.2).
//! [`LnoFile::matches`] is the LNO-4.1 check that a `.lno` belongs to a given `progs.dat`.
//!
//! The format is specified in `docs/spec/lno.md`; this crate implements LNO-2 to LNO-6. The
//! compiler-side statements LNO-1.1 to LNO-1.6 are about when a compiler writes a `.lno`; the
//! editor's own compiler follows LNO-6.4, and LNO-4.4 and LNO-4.5 describe how a reader of
//! line numbers finds a statement's file.

mod read;
mod write;

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod vectors;

pub use read::ReadError;
pub use write::WriteError;

/// The four magic bytes a `.lno` starts with, id's `LNOF`.
///
/// `LNO_MAGIC` in LNO-2.
pub const LNO_MAGIC: [u8; 4] = *b"LNOF";

/// The only format version any writer produces or any reader accepts.
///
/// `LNO_VERSION` in LNO-2.
pub const LNO_VERSION: u32 = 1;

/// Bytes in the fixed header before the line table: the magic and five 32-bit counts.
///
/// `LNO_HEADER_SIZE` in LNO-2.
pub const LNO_HEADER_SIZE: usize = 24;

/// The largest line table length, the largest `numstatements` a PROGS-2.1 `i32` can hold.
///
/// Used by the writer (LNO-6.3) to refuse a table it could not record. Equal to
/// [`i32::MAX`] as a count.
pub const LNO_MAX_STATEMENTS: usize = 2_147_483_647;

/// A parsed `.lno` file: the three counts it shares with its `progs.dat`, the line table, the
/// optional column table and any trailing bytes.
///
/// `numstatements` is not stored: it is the length of [`lines`](Self::lines), which the writer
/// records (LNO-6.1) and which a column table, when present, matches (LNO-6.3). The three
/// other counts are kept exactly as read (LNO-5.7) and written as given (LNO-6.1); they are
/// the LNO-4.1 check against a `progs.dat` and carry no meaning to this crate otherwise.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LnoFile {
    /// The progs header's `numglobaldefs` (LNO-2.3), kept as read.
    pub numglobaldefs: u32,
    /// The progs header's `numglobals` (LNO-2.3), kept as read.
    pub numglobals: u32,
    /// The progs header's `numfielddefs` (LNO-2.3), kept as read.
    pub numfielddefs: u32,
    /// The source line of each statement (LNO-3.1), one per statement, 1-based with 0 meaning
    /// the writer had no line (LNO-3.2). Values are signed and kept unchanged (LNO-3.5).
    pub lines: Vec<i32>,
    /// GMQCC's column of each statement (LNO-3.3, LNO-3.4) when the file has a column table,
    /// one per statement; [`None`] for FTEQCC's layout, which writes no column table.
    pub columns: Option<Vec<i32>>,
    /// Bytes after the last table the file has (LNO-5.5), kept so the writer reproduces the
    /// file (LNO-6.2). Empty for a file with nothing after its tables.
    pub trailing: Vec<u8>,
}

/// Which progs header field a [`LnoFile`] did not match, from [`LnoFile::matches`].
///
/// The fields are compared in header order (LNO-2) and the first that differs is reported
/// (LNO-4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CountField {
    /// `numglobaldefs` differs.
    GlobalDefs,
    /// `numglobals` differs.
    Globals,
    /// `numfielddefs` differs.
    FieldDefs,
    /// `numstatements` differs: the line table length is not the progs' statement count.
    Statements,
}

impl core::fmt::Display for CountField {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::GlobalDefs => "numglobaldefs",
            Self::Globals => "numglobals",
            Self::FieldDefs => "numfielddefs",
            Self::Statements => "numstatements",
        })
    }
}

/// A `.lno` did not belong to the `progs.dat` it was checked against (LNO-4.2).
///
/// Names the first header field that differs, in header order, with the `.lno`'s value and the
/// progs' value. The caller decides whether to drop the `.lno`; the readers of the spec do so
/// silently (LNO-4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatchError {
    /// The first field, in header order, whose values differ.
    pub field: CountField,
    /// The `.lno`'s value for that field.
    pub lno: u32,
    /// The `progs.dat`'s value for that field.
    pub progs: u32,
}

impl core::fmt::Display for MatchError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "the .lno's {} is {}, the progs.dat's is {}",
            self.field, self.lno, self.progs
        )
    }
}

impl core::error::Error for MatchError {}

impl LnoFile {
    /// The `numstatements` this file records: the length of the line table (LNO-6.1), capped
    /// at [`u32::MAX`]. A valid file read back by [`LnoFile::read`] always fits (its length
    /// came from a `u32` header); a file built by hand with more than [`u32::MAX`] lines is
    /// refused by [`LnoFile::write`] (LNO-6.3).
    #[must_use]
    pub fn numstatements(&self) -> u32 {
        u32::try_from(self.lines.len()).unwrap_or(u32::MAX)
    }

    /// Checks that this `.lno` belongs to a `progs.dat` with the given header counts (LNO-4.1):
    /// all four counts equal the progs' `numglobaldefs`, `numglobals`, `numfielddefs` and
    /// `numstatements`, compared as unsigned 32-bit values. Returns the first field that
    /// differs in header order (LNO-4.2).
    ///
    /// # Errors
    ///
    /// Returns a [`MatchError`] naming the first differing field.
    pub fn matches(
        &self,
        numglobaldefs: u32,
        numglobals: u32,
        numfielddefs: u32,
        numstatements: u32,
    ) -> Result<(), MatchError> {
        let pairs = [
            (CountField::GlobalDefs, self.numglobaldefs, numglobaldefs),
            (CountField::Globals, self.numglobals, numglobals),
            (CountField::FieldDefs, self.numfielddefs, numfielddefs),
            (CountField::Statements, self.numstatements(), numstatements),
        ];
        for (field, lno, progs) in pairs {
            if lno != progs {
                return Err(MatchError { field, lno, progs });
            }
        }
        Ok(())
    }

    /// Checks that this `.lno` belongs to `header`'s `progs.dat` (LNO-4.1), reading the four
    /// counts from a [`qc_progs::Header`]. The header stores them as `i32`; they are compared
    /// as the unsigned 32-bit values the spec uses (FTE and DarkPlaces both compare unsigned).
    ///
    /// # Errors
    ///
    /// Returns a [`MatchError`] naming the first differing field.
    pub fn matches_header(&self, header: &qc_progs::Header) -> Result<(), MatchError> {
        self.matches(
            header.numglobaldefs.cast_unsigned(),
            header.numglobals.cast_unsigned(),
            header.numfielddefs.cast_unsigned(),
            header.numstatements.cast_unsigned(),
        )
    }
}
