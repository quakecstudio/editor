// SPDX-License-Identifier: GPL-3.0-or-later

//! Reads, validates and writes `progs.dat`, the compiled QuakeC file.
//!
//! [`Progs::read`] is id's `PR_LoadProgs`: it checks a file in the order of PROGS-15.5 and
//! returns a zero-copy view whose tables decode from the file's bytes on access. Errors are
//! [`ReadError`] values, never panics, for any input. [`ProgsFile`] is an owned copy that
//! [`write()`] turns back into bytes; a file read and written back is the same byte for byte
//! (PROGS-22.1). The header's version decides the format: 6 is id's, and 7 is FTEQCC's
//! (PROGS-19 to PROGS-21).
//!
//! The format is specified in `docs/spec/progs.md` and its limits in `docs/spec/limits.md`;
//! [`limits`] names them all.

pub mod crc;
pub mod limits;
pub mod opcode;
mod read;
mod records;
mod write;

#[cfg(test)]
mod fixtures;

pub use read::{Progs, ReadError, ReadOptions, StringSite, Table, TableIter};
pub use records::{
    Def, Format, Function, Header, PROGS_DEF_SAVEGLOBAL, PROGS_DEF_SIZE, PROGS_FUNCTION_SIZE,
    PROGS_GLOBAL_SIZE, PROGS_HEADER_CRC, PROGS_HEADER_SIZE, PROGS_STATEMENT_SIZE,
    PROGS_STRING_ALIGN, PROGS_VERSION, Record, Statement,
};
pub use records::{
    FteHeader, PROGS_FTE_HEADER_SIZE, PROGS_FTE_LINE_SIZE, PROGS_FTE_SECONDARY16,
    PROGS_FTE_SECONDARY32, PROGS_FTE_TYPE_SIZE, PROGS_FTE32_DEF_SIZE, PROGS_FTE32_STATEMENT_SIZE,
    PROGS_VERSION_FTE,
};
pub use write::FteParts;
pub use write::{ProgsFile, WriteError, write};

/// One of the tables a progs.dat locates through its header.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TableId {
    /// The statements (PROGS-7).
    Statements,
    /// The global definitions (PROGS-9).
    GlobalDefs,
    /// The field definitions (PROGS-9).
    FieldDefs,
    /// The functions (PROGS-10).
    Functions,
    /// The string table (PROGS-11).
    Strings,
    /// The globals (PROGS-12).
    Globals,
    /// FTE's line numbers (PROGS-21.1).
    LineNumbers,
    /// FTE's names of functions without bodies (PROGS-21.2).
    BodylessFunctions,
}

impl core::fmt::Display for TableId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Statements => "statements",
            Self::GlobalDefs => "global definitions",
            Self::FieldDefs => "field definitions",
            Self::Functions => "functions",
            Self::Strings => "strings",
            Self::Globals => "globals",
            Self::LineNumbers => "line numbers",
            Self::BodylessFunctions => "bodyless functions",
        })
    }
}

/// Where a file's tables lie and the bytes between them, from [`Progs::layout`].
///
/// Implements PROGS-22.1: [`write()`] places the tables in this order with these bytes between
/// them, so a file read and written back is the same byte for byte.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    /// Bytes between the header and the first table.
    lead: Vec<u8>,
    /// The non-empty tables in file order.
    tables: Vec<LayoutTable>,
    /// The stored offset of every empty table.
    empty: Vec<(TableId, u32)>,
}

/// One non-empty table of a [`Layout`] and the bytes after it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct LayoutTable {
    table: TableId,
    gap_after: Vec<u8>,
}
