// SPDX-License-Identifier: GPL-3.0-or-later

//! Writing an [`LnoFile`] back to bytes.
//!
//! Implements LNO-6 of `docs/spec/lno.md`: the writer lays out the header, line table,
//! optional column table and trailing bytes (LNO-6.1), rejects a file it could not read back
//! unchanged (LNO-6.3), and a file the reader accepts is written back byte for byte (LNO-6.2).

use crate::{LNO_HEADER_SIZE, LNO_MAGIC, LNO_MAX_STATEMENTS, LNO_VERSION, LnoFile};

/// Why an [`LnoFile`] could not be written. Each variant is one of the LNO-6.3 checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteError {
    /// The column table's length differs from the line table's (LNO-6.3): the two lengths.
    ColumnCountMismatch {
        /// The number of lines.
        lines: usize,
        /// The number of columns.
        columns: usize,
    },
    /// A column table was given for an empty line table (LNO-6.3): the reader would read it
    /// back as no column table (LNO-5.4), so the writer refuses it.
    ColumnsWithoutLines,
    /// The line table has more than [`LNO_MAX_STATEMENTS`] entries (LNO-6.3): its length.
    TooManyStatements(usize),
}

impl core::fmt::Display for WriteError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ColumnCountMismatch { lines, columns } => write!(
                f,
                "the column table has {columns} entries, the line table has {lines}"
            ),
            Self::ColumnsWithoutLines => {
                f.write_str("a column table was given for an empty line table")
            }
            Self::TooManyStatements(len) => write!(
                f,
                "the line table has {len} entries, more than the {LNO_MAX_STATEMENTS} a .lno can record"
            ),
        }
    }
}

impl core::error::Error for WriteError {}

impl LnoFile {
    /// Writes this `.lno` back to bytes (LNO-6).
    ///
    /// Lays out `LNOF`, version 1, the three counts as given and `numstatements` as the line
    /// table length, the line table, the column table if there is one, then the trailing bytes
    /// (LNO-6.1). A file produced by [`LnoFile::read`] is written back byte for byte (LNO-6.2).
    ///
    /// # Errors
    ///
    /// Returns a [`WriteError`] (LNO-6.3) when the column table length differs from the line
    /// table's, when a column table is given for an empty line table, or when the line table
    /// has more than [`LNO_MAX_STATEMENTS`] entries. Nothing is written in that case.
    pub fn write(&self) -> Result<Vec<u8>, WriteError> {
        // spec LNO-6.3: the writer refuses a line table it could not record.
        if self.lines.len() > LNO_MAX_STATEMENTS {
            return Err(WriteError::TooManyStatements(self.lines.len()));
        }
        if let Some(columns) = &self.columns {
            // spec LNO-6.3: a column table whose length differs from the line table's.
            if columns.len() != self.lines.len() {
                return Err(WriteError::ColumnCountMismatch {
                    lines: self.lines.len(),
                    columns: columns.len(),
                });
            }
            // spec LNO-6.3: a column table when the line table is empty.
            if self.lines.is_empty() {
                return Err(WriteError::ColumnsWithoutLines);
            }
        }

        let n = self.numstatements();
        let mut out = Vec::with_capacity(self.size_hint());
        // spec LNO-2.1: the 24-byte header is `LNOF`, then five 32-bit little-endian integers.
        out.extend_from_slice(&LNO_MAGIC);
        out.extend_from_slice(&LNO_VERSION.to_le_bytes());
        out.extend_from_slice(&self.numglobaldefs.to_le_bytes());
        out.extend_from_slice(&self.numglobals.to_le_bytes());
        out.extend_from_slice(&self.numfielddefs.to_le_bytes());
        // spec LNO-6.1: numstatements is the length of the line table.
        out.extend_from_slice(&n.to_le_bytes());
        // spec LNO-3.1: the line table, one i32 per statement.
        for line in &self.lines {
            out.extend_from_slice(&line.to_le_bytes());
        }
        // spec LNO-3.3: the column table, when present, follows the line table.
        if let Some(columns) = &self.columns {
            for column in columns {
                out.extend_from_slice(&column.to_le_bytes());
            }
        }
        // spec LNO-6.1: the trailing bytes, if any, go last.
        out.extend_from_slice(&self.trailing);
        Ok(out)
    }

    /// The exact number of bytes [`LnoFile::write`] will produce for a file it accepts: the
    /// header, the line table, the column table if any and the trailing bytes. Saturates
    /// rather than overflowing; a file that would overflow is refused by `write` first.
    fn size_hint(&self) -> usize {
        let per_statement = if self.columns.is_some() { 8 } else { 4 };
        self.lines
            .len()
            .checked_mul(per_statement)
            .and_then(|tables| tables.checked_add(LNO_HEADER_SIZE))
            .and_then(|head| head.checked_add(self.trailing.len()))
            .unwrap_or(usize::MAX)
    }
}
