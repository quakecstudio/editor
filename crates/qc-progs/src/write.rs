// SPDX-License-Identifier: GPL-3.0-or-later

//! The writer: an owned progs.dat and the bytes it is written as.
//!
//! Implements PROGS-22 of `docs/spec/progs.md`. The table order without a source layout is
//! the order of id's qcc (`WriteData` in `qcc/qcc.c`, PROGS-1.3).

use core::fmt;

use qc_types::Slot;

use crate::records::{
    Def, Format, Function, Header, PROGS_HEADER_CRC, PROGS_STRING_ALIGN, Record, Statement,
};
use crate::{Layout, Progs, TableId};

#[cfg(test)]
mod tests;

/// The parts of a version 7 file beyond the version 6 tables (PROGS-19, PROGS-21).
#[cfg(feature = "fte")]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FteParts {
    /// The line number of each statement, or `None` when the file has no table (PROGS-21.1).
    pub line_numbers: Option<Vec<i32>>,
    /// The names of functions without bodies, without their NULs (PROGS-21.2).
    pub bodyless_functions: Vec<Vec<u8>>,
    /// `ofsfiles`, kept as stored and not decoded (PROGS-21.5).
    pub ofsfiles: u32,
    /// `ofs_types`, kept as stored and not decoded (PROGS-21.5).
    pub ofs_types: u32,
    /// `numtypes`, kept as stored (PROGS-21.5).
    pub numtypes: u32,
}

/// An owned progs.dat: every header value [`write()`] does not compute, and every table.
#[derive(Clone, Debug, PartialEq)]
pub struct ProgsFile {
    /// The layout to write; it sets `version` and, for version 7, `secondaryversion`.
    pub format: Format,
    /// The header CRC (PROGS-5).
    pub crc: i32,
    /// Field slots in every entity (PROGS-13).
    pub entityfields: i32,
    /// The statements (PROGS-7).
    pub statements: Vec<Statement>,
    /// The global definitions (PROGS-9).
    pub global_defs: Vec<Def>,
    /// The field definitions (PROGS-9).
    pub field_defs: Vec<Def>,
    /// The functions (PROGS-10).
    pub functions: Vec<Function>,
    /// The string table's bytes, padding included (PROGS-11).
    pub strings: Vec<u8>,
    /// The globals (PROGS-12).
    pub globals: Vec<Slot>,
    /// The version 7 parts; must be empty for a version 6 file.
    #[cfg(feature = "fte")]
    pub fte: FteParts,
    /// Where the file this came from had its tables (PROGS-22.1), or `None` for qcc's layout
    /// (PROGS-22.2).
    pub layout: Option<Layout>,
}

impl Default for ProgsFile {
    /// An empty version 6 file with id's header CRC, written in qcc's layout.
    fn default() -> Self {
        Self {
            format: Format::V6,
            crc: PROGS_HEADER_CRC,
            entityfields: 0,
            statements: Vec::new(),
            global_defs: Vec::new(),
            field_defs: Vec::new(),
            functions: Vec::new(),
            strings: Vec::new(),
            globals: Vec::new(),
            #[cfg(feature = "fte")]
            fte: FteParts::default(),
            layout: None,
        }
    }
}

impl Progs<'_> {
    /// An owned copy of every table and of the layout, for editing and [`write()`].
    #[must_use]
    pub fn to_file(&self) -> ProgsFile {
        let header = self.header();
        ProgsFile {
            format: self.format(),
            crc: header.crc,
            entityfields: header.entityfields,
            statements: self.statements().iter().collect(),
            global_defs: self.global_defs().iter().collect(),
            field_defs: self.field_defs().iter().collect(),
            functions: self.functions().iter().collect(),
            strings: self.strings().to_vec(),
            globals: self.globals().iter().collect(),
            #[cfg(feature = "fte")]
            fte: self
                .fte_header()
                .map_or_else(FteParts::default, |fte| FteParts {
                    line_numbers: self.line_numbers().map(|table| table.iter().collect()),
                    bodyless_functions: self.bodyless_functions().map(<[u8]>::to_vec).collect(),
                    ofsfiles: fte.ofsfiles,
                    ofs_types: fte.ofs_types,
                    numtypes: fte.numtypes,
                }),
            layout: self.layout(),
        }
    }
}

/// Why [`write()`] could not write a [`ProgsFile`] (PROGS-22.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WriteError {
    /// A statement's opcode or an operand does not fit the format's field.
    StatementOutOfRange {
        /// The statement's index.
        index: usize,
    },
    /// A definition's type or slot does not fit the format's field.
    DefOutOfRange {
        /// The global or field definitions.
        table: TableId,
        /// The definition's index.
        index: usize,
    },
    /// A table's count or offset does not fit the header's `i32`.
    TooLarge {
        /// The table.
        table: TableId,
    },
    /// Version 7 parts on a version 6 file.
    #[cfg(feature = "fte")]
    FtePartsInV6,
    /// The number of line numbers is not the number of statements (PROGS-21.1).
    #[cfg(feature = "fte")]
    LineNumberCount {
        /// Line numbers given.
        line_numbers: usize,
        /// Statements given.
        statements: usize,
    },
    /// A bodyless function name contains a NUL byte (PROGS-21.2).
    #[cfg(feature = "fte")]
    NulInBodylessName {
        /// The name's index.
        index: usize,
    },
    /// `ofsfiles` or `ofs_types` is not 0 but there is no source layout to keep the bytes it
    /// points at (PROGS-21.5).
    #[cfg(feature = "fte")]
    OpaqueTableWithoutLayout,
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StatementOutOfRange { index } => {
                write!(f, "statement {index} does not fit the progs.dat format")
            }
            Self::DefOutOfRange { table, index } => {
                write!(f, "{table} entry {index} does not fit the progs.dat format")
            }
            Self::TooLarge { table } => write!(f, "the {table} table is too large for progs.dat"),
            #[cfg(feature = "fte")]
            Self::FtePartsInV6 => f.write_str("a version 6 progs.dat cannot hold FTE tables"),
            #[cfg(feature = "fte")]
            Self::LineNumberCount {
                line_numbers,
                statements,
            } => write!(
                f,
                "{line_numbers} line numbers given for {statements} statements"
            ),
            #[cfg(feature = "fte")]
            Self::NulInBodylessName { index } => {
                write!(f, "bodyless function name {index} contains a NUL byte")
            }
            #[cfg(feature = "fte")]
            Self::OpaqueTableWithoutLayout => f.write_str(
                "ofsfiles or ofs_types points into a source file whose layout is not kept",
            ),
        }
    }
}

impl core::error::Error for WriteError {}

/// One table's bytes, ready to place.
struct Encoded {
    table: TableId,
    bytes: Vec<u8>,
    offset: Option<u32>,
}

/// Writes `file` as a progs.dat.
///
/// Implements PROGS-22: the tables go where [`ProgsFile::layout`] says (PROGS-22.1), or in
/// qcc's order (PROGS-22.2); the header's counts and offsets come from the tables
/// (PROGS-22.3).
///
/// # Errors
///
/// A [`WriteError`] when a value does not fit the format (PROGS-22.4).
pub fn write(file: &ProgsFile) -> Result<Vec<u8>, WriteError> {
    #[cfg(feature = "fte")]
    if file.format == Format::V6 && file.fte != FteParts::default() {
        return Err(WriteError::FtePartsInV6);
    }
    let mut encoded = encode_tables(file)?;
    let mut out = vec![0_u8; file.format.header_size()];
    match &file.layout {
        Some(layout) => place_in_layout(&mut out, &mut encoded, layout),
        None => place_in_qcc_order(&mut out, &mut encoded, file)?,
    }
    let raw_offset = |table: TableId| {
        encoded
            .iter()
            .find(|entry| entry.table == table)
            .and_then(|entry| entry.offset)
            .unwrap_or_default()
    };
    let offset = |table: TableId| -> Result<i32, WriteError> {
        i32::try_from(raw_offset(table)).map_err(|_| WriteError::TooLarge { table })
    };
    let count = |table: TableId, len: usize| -> Result<i32, WriteError> {
        i32::try_from(len).map_err(|_| WriteError::TooLarge { table })
    };
    let numstrings = encoded
        .iter()
        .find(|entry| entry.table == TableId::Strings)
        .map_or(0, |entry| entry.bytes.len());
    let header = Header {
        version: file.format.version(),
        crc: file.crc,
        ofs_statements: offset(TableId::Statements)?,
        numstatements: count(TableId::Statements, file.statements.len())?,
        ofs_globaldefs: offset(TableId::GlobalDefs)?,
        numglobaldefs: count(TableId::GlobalDefs, file.global_defs.len())?,
        ofs_fielddefs: offset(TableId::FieldDefs)?,
        numfielddefs: count(TableId::FieldDefs, file.field_defs.len())?,
        ofs_functions: offset(TableId::Functions)?,
        numfunctions: count(TableId::Functions, file.functions.len())?,
        ofs_strings: offset(TableId::Strings)?,
        numstrings: count(TableId::Strings, numstrings)?,
        ofs_globals: offset(TableId::Globals)?,
        numglobals: count(TableId::Globals, file.globals.len())?,
        entityfields: file.entityfields,
    };
    #[cfg_attr(
        not(feature = "fte"),
        expect(unused_mut, reason = "FTE fields are added")
    )]
    let mut fields: Vec<u8> = header
        .fields()
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    #[cfg(feature = "fte")]
    if let Some(secondaryversion) = file.format.secondary_version() {
        let numbodylessfuncs =
            u32::try_from(file.fte.bodyless_functions.len()).map_err(|_| WriteError::TooLarge {
                table: TableId::BodylessFunctions,
            })?;
        let extra = [
            file.fte.ofsfiles,
            raw_offset(TableId::LineNumbers),
            raw_offset(TableId::BodylessFunctions),
            numbodylessfuncs,
            file.fte.ofs_types,
            file.fte.numtypes,
            0,
        ];
        fields.extend(extra.iter().flat_map(|v| v.to_le_bytes()));
        fields.extend(secondaryversion.to_le_bytes());
    }
    for (slot, byte) in out.iter_mut().zip(fields) {
        *slot = byte;
    }
    Ok(out)
}

/// PROGS-22.1: the tables in the source file's order with its bytes between them; an empty
/// table keeps its stored offset, and a table the source did not have goes at the end.
fn place_in_layout(out: &mut Vec<u8>, encoded: &mut [Encoded], layout: &Layout) {
    out.extend_from_slice(&layout.lead);
    for placed in &layout.tables {
        if let Some(entry) = encoded.iter_mut().find(|entry| entry.table == placed.table) {
            place(out, entry);
        }
        out.extend_from_slice(&placed.gap_after);
    }
    for (table, stored) in &layout.empty {
        if let Some(entry) = encoded
            .iter_mut()
            .find(|entry| entry.table == *table && entry.offset.is_none())
            .filter(|entry| entry.bytes.is_empty())
        {
            entry.offset = Some(*stored);
        }
    }
    for entry in encoded.iter_mut().filter(|entry| entry.offset.is_none()) {
        place(out, entry);
    }
}

/// PROGS-22.2: qcc's order, the string table padded to [`PROGS_STRING_ALIGN`].
fn place_in_qcc_order(
    out: &mut Vec<u8>,
    encoded: &mut [Encoded],
    file: &ProgsFile,
) -> Result<(), WriteError> {
    #[cfg(feature = "fte")]
    if file.fte.ofsfiles != 0 || file.fte.ofs_types != 0 {
        return Err(WriteError::OpaqueTableWithoutLayout);
    }
    #[cfg(not(feature = "fte"))]
    let _ = file;
    for entry in encoded.iter_mut() {
        if entry.table == TableId::Strings {
            while !entry.bytes.len().is_multiple_of(PROGS_STRING_ALIGN) {
                entry.bytes.push(0);
            }
        }
        #[cfg(feature = "fte")]
        if entry.table == TableId::BodylessFunctions && entry.bytes.is_empty() {
            entry.offset = Some(0);
            continue;
        }
        place(out, entry);
    }
    Ok(())
}

fn place(out: &mut Vec<u8>, entry: &mut Encoded) {
    entry.offset = Some(u32::try_from(out.len()).unwrap_or(u32::MAX));
    out.extend_from_slice(&entry.bytes);
}

/// Every table's bytes, in qcc's order (PROGS-1.3), with the checks of PROGS-22.4.
fn encode_tables(file: &ProgsFile) -> Result<Vec<Encoded>, WriteError> {
    let format = file.format;
    let mut statements = Vec::new();
    for (index, statement) in file.statements.iter().enumerate() {
        statement
            .encode(&mut statements, format)
            .ok_or(WriteError::StatementOutOfRange { index })?;
    }
    let defs = |table: TableId, defs: &[Def]| -> Result<Vec<u8>, WriteError> {
        let mut bytes = Vec::new();
        for (index, def) in defs.iter().enumerate() {
            def.encode(&mut bytes, format)
                .ok_or(WriteError::DefOutOfRange { table, index })?;
        }
        Ok(bytes)
    };
    let mut functions = Vec::new();
    for function in &file.functions {
        function.encode(&mut functions, format);
    }
    let mut globals = Vec::new();
    for slot in &file.globals {
        slot.encode(&mut globals, format);
    }
    let entry = |table, bytes| Encoded {
        table,
        bytes,
        offset: None,
    };
    #[cfg_attr(
        not(feature = "fte"),
        expect(unused_mut, reason = "FTE tables are added")
    )]
    let mut tables = vec![
        entry(TableId::Strings, file.strings.clone()),
        entry(TableId::Statements, statements),
        entry(TableId::Functions, functions),
        entry(
            TableId::GlobalDefs,
            defs(TableId::GlobalDefs, &file.global_defs)?,
        ),
        entry(
            TableId::FieldDefs,
            defs(TableId::FieldDefs, &file.field_defs)?,
        ),
        entry(TableId::Globals, globals),
    ];
    #[cfg(feature = "fte")]
    if format != Format::V6 {
        if let Some(lines) = &file.fte.line_numbers {
            if lines.len() != file.statements.len() {
                return Err(WriteError::LineNumberCount {
                    line_numbers: lines.len(),
                    statements: file.statements.len(),
                });
            }
            let mut bytes = Vec::new();
            for line in lines {
                line.encode(&mut bytes, format);
            }
            tables.push(entry(TableId::LineNumbers, bytes));
        }
        let mut names = Vec::new();
        for (index, name) in file.fte.bodyless_functions.iter().enumerate() {
            if name.contains(&0) {
                return Err(WriteError::NulInBodylessName { index });
            }
            names.extend_from_slice(name);
            names.push(0);
        }
        tables.push(entry(TableId::BodylessFunctions, names));
    }
    Ok(tables)
}
