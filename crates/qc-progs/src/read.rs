// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_edict.c, PR_LoadProgs, ED_GlobalAtOfs, ED_FieldAtOfs, ED_FindField, ED_FindGlobal, ED_FindFunction

//! The zero-copy reader: id's `PR_LoadProgs` with the checks of PROGS-15, and the lookups of
//! `pr_edict.c` that find a definition or function by name or slot.

use core::fmt;
use core::marker::PhantomData;
use core::ops::Range;

use qc_types::Slot;

use crate::crc::crc_block;
use crate::limits::LIMIT_MAX_PARMS;
use crate::records::{
    Def, Format, Function, Header, PROGS_HEADER_CRC, PROGS_HEADER_SIZE, PROGS_VERSION, Record,
    Statement,
};
#[cfg(feature = "fte")]
use crate::records::{
    FteHeader, PROGS_FTE_HEADER_SIZE, PROGS_FTE_LINE_SIZE, PROGS_FTE_SECONDARY16,
    PROGS_FTE_SECONDARY32, PROGS_VERSION_FTE,
};
use crate::{Layout, LayoutTable, TableId};

#[cfg(test)]
mod tests;

/// Options for [`Progs::read_with`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadOptions {
    /// Reject a file whose header CRC is not [`PROGS_HEADER_CRC`] (PROGS-3.2). Turning it off
    /// opens a mod whose system definitions differ from id's (PROGS-15.8).
    pub check_header_crc: bool,
}

impl Default for ReadOptions {
    /// Checks the header CRC, as id's engine does (PROGS-15.8).
    fn default() -> Self {
        Self {
            check_header_crc: true,
        }
    }
}

/// Where a string reference that failed PROGS-15.3 or PROGS-15.4 was.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StringSite {
    /// `s_name` of global definition `index`.
    GlobalDefName(usize),
    /// `s_name` of field definition `index`.
    FieldDefName(usize),
    /// `s_name` of function `index`.
    FunctionName(usize),
    /// `s_file` of function `index`.
    FunctionFile(usize),
}

impl fmt::Display for StringSite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GlobalDefName(index) => write!(f, "the name of global definition {index}"),
            Self::FieldDefName(index) => write!(f, "the name of field definition {index}"),
            Self::FunctionName(index) => write!(f, "the name of function {index}"),
            Self::FunctionFile(index) => write!(f, "the file name of function {index}"),
        }
    }
}

/// Why a file was rejected, in the order PROGS-15.5 checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReadError {
    /// The file is shorter than its header (PROGS-15.1, PROGS-21.9).
    TooShort {
        /// Bytes in the file.
        len: usize,
        /// Bytes in the header.
        needed: usize,
    },
    /// `version` is not 6, or with the `fte` feature 7 (PROGS-3.1).
    WrongVersion {
        /// The stored version.
        version: i32,
    },
    /// `crc` is not 5927 (PROGS-3.2).
    HeaderCrc {
        /// The stored CRC.
        crc: i32,
    },
    /// A version 7 `secondaryversion` other than FTE's 16-bit or 32-bit layout (PROGS-19.3).
    #[cfg(feature = "fte")]
    UnsupportedFteLayout {
        /// The stored `secondaryversion`.
        secondaryversion: i32,
    },
    /// A version 7 file with compressed tables (PROGS-21.4).
    #[cfg(feature = "fte")]
    Compressed {
        /// The stored `blockscompressed`.
        blockscompressed: u32,
    },
    /// A table's offset or count is negative, or it ends past the end of the file
    /// (PROGS-15.2).
    TableOutOfBounds {
        /// The table.
        table: TableId,
    },
    /// A field definition has `DEF_SAVEGLOBAL` set (PROGS-3.4).
    FieldDefSaveGlobal {
        /// The field definition's index.
        index: usize,
    },
    /// A string reference is negative or not less than `numstrings` (PROGS-15.3).
    StringOutOfRange {
        /// Where the reference is.
        site: StringSite,
        /// The reference.
        offset: i32,
    },
    /// A string reference reaches the end of the string table without a NUL (PROGS-15.4).
    UnterminatedString {
        /// Where the reference is.
        site: StringSite,
        /// The reference.
        offset: i32,
    },
    /// A function has more than 8 parameters (PROGS-15.6).
    TooManyParms {
        /// The function's index.
        index: usize,
        /// Its `numparms`.
        numparms: i32,
    },
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort { len, needed } => {
                write!(
                    f,
                    "progs.dat is {len} bytes, shorter than its {needed}-byte header"
                )
            }
            Self::WrongVersion { version } => write!(
                f,
                "progs.dat has wrong version number ({version} should be {PROGS_VERSION})"
            ),
            Self::HeaderCrc { .. } => {
                f.write_str("progs.dat system vars have been modified, progdefs.h is out of date")
            }
            #[cfg(feature = "fte")]
            Self::UnsupportedFteLayout { secondaryversion } => write!(
                f,
                "progs.dat version 7 layout {secondaryversion:#010x} is not FTE's 16-bit or \
                 32-bit layout"
            ),
            #[cfg(feature = "fte")]
            Self::Compressed { blockscompressed } => write!(
                f,
                "progs.dat has compressed tables (blockscompressed {blockscompressed:#x})"
            ),
            Self::TableOutOfBounds { table } => {
                write!(f, "progs.dat {table} table lies outside the file")
            }
            Self::FieldDefSaveGlobal { .. } => {
                f.write_str("PR_LoadProgs: pr_fielddefs[i].type & DEF_SAVEGLOBAL")
            }
            Self::StringOutOfRange { site, offset } => {
                write!(f, "progs.dat {site} is {offset}, outside the string table")
            }
            Self::UnterminatedString { site, offset } => write!(
                f,
                "progs.dat {site} at {offset} runs to the end of the string table without a NUL"
            ),
            Self::TooManyParms { index, numparms } => write!(
                f,
                "progs.dat function {index} has {numparms} parameters, more than 8"
            ),
        }
    }
}

impl core::error::Error for ReadError {}

/// A zero-copy view of one table: its records decode from the file's bytes on access.
#[derive(Debug)]
pub struct Table<'a, T> {
    bytes: &'a [u8],
    len: usize,
    format: Format,
    record: PhantomData<fn() -> T>,
}

impl<T> Clone for Table<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Table<'_, T> {}

impl<'a, T: Record> Table<'a, T> {
    fn new(bytes: &'a [u8], len: usize, format: Format) -> Self {
        Self {
            bytes,
            len,
            format,
            record: PhantomData,
        }
    }

    /// Number of records.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether the table has no records.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Record `index`, or `None` past the end.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<T> {
        let size = T::size(self.format);
        let start = index.checked_mul(size)?;
        let chunk = self.bytes.get(start..start.checked_add(size)?)?;
        T::decode(chunk, self.format)
    }

    /// The table's bytes as stored.
    #[must_use]
    pub const fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Every record, in table order.
    #[must_use]
    pub fn iter(&self) -> TableIter<'a, T> {
        TableIter {
            table: *self,
            next: 0,
        }
    }
}

impl<'a, T: Record> IntoIterator for Table<'a, T> {
    type Item = T;
    type IntoIter = TableIter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Iterator over a [`Table`]'s records.
#[derive(Clone, Debug)]
pub struct TableIter<'a, T> {
    table: Table<'a, T>,
    next: usize,
}

impl<T: Record> Iterator for TableIter<'_, T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        let record = self.table.get(self.next)?;
        self.next = self.next.saturating_add(1);
        Some(record)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = self.table.len.saturating_sub(self.next);
        (left, Some(left))
    }
}

impl<T: Record> ExactSizeIterator for TableIter<'_, T> {}

/// Where each table lies in the file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Extents {
    statements: Range<usize>,
    global_defs: Range<usize>,
    field_defs: Range<usize>,
    functions: Range<usize>,
    strings: Range<usize>,
    globals: Range<usize>,
    #[cfg(feature = "fte")]
    line_numbers: Option<Range<usize>>,
    #[cfg(feature = "fte")]
    bodyless: Range<usize>,
}

/// A loaded progs.dat: the header and a zero-copy view of every table.
///
/// Implements PROGS-1 to PROGS-15, and with the `fte` feature PROGS-19 to PROGS-21. Every
/// record reads back exactly as stored (PROGS-14).
#[derive(Clone, Debug)]
pub struct Progs<'a> {
    bytes: &'a [u8],
    format: Format,
    header: Header,
    #[cfg(feature = "fte")]
    fte_header: Option<FteHeader>,
    extents: Extents,
}

impl<'a> Progs<'a> {
    /// Loads a progs.dat with the default options, id's `PR_LoadProgs`.
    ///
    /// # Errors
    ///
    /// The first check of PROGS-15.5 the file fails, as a [`ReadError`].
    pub fn read(bytes: &'a [u8]) -> Result<Self, ReadError> {
        Self::read_with(bytes, &ReadOptions::default())
    }

    /// Loads a progs.dat, id's `PR_LoadProgs`, with the checks of PROGS-15 in the order of
    /// PROGS-15.5, PROGS-15.9 and PROGS-21.9.
    ///
    /// # Errors
    ///
    /// The first check the file fails, as a [`ReadError`].
    pub fn read_with(bytes: &'a [u8], options: &ReadOptions) -> Result<Self, ReadError> {
        let too_short = |needed| ReadError::TooShort {
            len: bytes.len(),
            needed,
        };
        let header = Header::decode(bytes).ok_or_else(|| too_short(PROGS_HEADER_SIZE))?;
        let format = match header.version {
            PROGS_VERSION => Format::V6,
            #[cfg(feature = "fte")]
            PROGS_VERSION_FTE => {
                if bytes.len() < PROGS_FTE_HEADER_SIZE {
                    return Err(too_short(PROGS_FTE_HEADER_SIZE));
                }
                // Resolved to the 16-bit or 32-bit layout once the CRC is checked.
                Format::Fte16
            }
            version => return Err(ReadError::WrongVersion { version }),
        };
        if options.check_header_crc && header.crc != PROGS_HEADER_CRC {
            return Err(ReadError::HeaderCrc { crc: header.crc });
        }
        #[cfg(feature = "fte")]
        let (format, fte_header) = fte_layout(bytes, format)?;

        let extents = Extents {
            statements: extent(
                bytes,
                TableId::Statements,
                header.ofs_statements,
                header.numstatements,
                format.statement_size(),
            )?,
            global_defs: extent(
                bytes,
                TableId::GlobalDefs,
                header.ofs_globaldefs,
                header.numglobaldefs,
                format.def_size(),
            )?,
            field_defs: extent(
                bytes,
                TableId::FieldDefs,
                header.ofs_fielddefs,
                header.numfielddefs,
                format.def_size(),
            )?,
            functions: extent(
                bytes,
                TableId::Functions,
                header.ofs_functions,
                header.numfunctions,
                Function::size(format),
            )?,
            strings: extent(
                bytes,
                TableId::Strings,
                header.ofs_strings,
                header.numstrings,
                1,
            )?,
            globals: extent(
                bytes,
                TableId::Globals,
                header.ofs_globals,
                header.numglobals,
                Slot::size(format),
            )?,
            #[cfg(feature = "fte")]
            line_numbers: fte_line_numbers(bytes, fte_header.as_ref(), header.numstatements)?,
            #[cfg(feature = "fte")]
            bodyless: fte_bodyless(bytes, fte_header.as_ref())?,
        };
        let progs = Self {
            bytes,
            format,
            header,
            #[cfg(feature = "fte")]
            fte_header,
            extents,
        };
        progs.check_field_defs()?;
        progs.check_string_refs()?;
        progs.check_parm_counts()?;
        Ok(progs)
    }

    /// PROGS-3.4: no field definition has `DEF_SAVEGLOBAL`.
    fn check_field_defs(&self) -> Result<(), ReadError> {
        for (index, def) in self.field_defs().iter().enumerate() {
            if def.is_save_global() {
                return Err(ReadError::FieldDefSaveGlobal { index });
            }
        }
        Ok(())
    }

    /// PROGS-15.3 and PROGS-15.4, in the order of PROGS-15.9.
    fn check_string_refs(&self) -> Result<(), ReadError> {
        for (index, def) in self.global_defs().iter().enumerate() {
            self.check_string_ref(StringSite::GlobalDefName(index), def.s_name)?;
        }
        for (index, def) in self.field_defs().iter().enumerate() {
            self.check_string_ref(StringSite::FieldDefName(index), def.s_name)?;
        }
        for (index, function) in self.functions().iter().enumerate() {
            self.check_string_ref(StringSite::FunctionName(index), function.s_name)?;
            self.check_string_ref(StringSite::FunctionFile(index), function.s_file)?;
        }
        Ok(())
    }

    fn check_string_ref(&self, site: StringSite, offset: i32) -> Result<(), ReadError> {
        let tail = usize::try_from(offset)
            .ok()
            .and_then(|start| self.strings().get(start..))
            .filter(|tail| !tail.is_empty())
            .ok_or(ReadError::StringOutOfRange { site, offset })?;
        if tail.contains(&0) {
            Ok(())
        } else {
            Err(ReadError::UnterminatedString { site, offset })
        }
    }

    /// PROGS-15.6: no function has more than 8 parameters.
    fn check_parm_counts(&self) -> Result<(), ReadError> {
        let max = i32::from(LIMIT_MAX_PARMS);
        for (index, function) in self.functions().iter().enumerate() {
            if function.numparms > max {
                return Err(ReadError::TooManyParms {
                    index,
                    numparms: function.numparms,
                });
            }
        }
        Ok(())
    }

    /// The whole file as loaded.
    #[must_use]
    pub const fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// The file's layout.
    #[must_use]
    pub const fn format(&self) -> Format {
        self.format
    }

    /// The version 6 header fields as stored (PROGS-2.1).
    #[must_use]
    pub const fn header(&self) -> &Header {
        &self.header
    }

    /// The version 7 header fields as stored (PROGS-19.1), for a version 7 file.
    #[cfg(feature = "fte")]
    #[must_use]
    pub const fn fte_header(&self) -> Option<&FteHeader> {
        self.fte_header.as_ref()
    }

    /// PROGS-4's CRC of every byte of the file, id's `pr_crc` (PROGS-6.1).
    #[must_use]
    pub fn file_crc(&self) -> u16 {
        crc_block(self.bytes)
    }

    fn slice(&self, range: &Range<usize>) -> &'a [u8] {
        self.bytes.get(range.clone()).unwrap_or_default()
    }

    fn table<T: Record>(&self, range: &Range<usize>) -> Table<'a, T> {
        let bytes = self.slice(range);
        let len = bytes
            .len()
            .checked_div(T::size(self.format))
            .unwrap_or_default();
        Table::new(bytes, len, self.format)
    }

    /// The statements (PROGS-7).
    #[must_use]
    pub fn statements(&self) -> Table<'a, Statement> {
        self.table(&self.extents.statements)
    }

    /// The global definitions (PROGS-9).
    #[must_use]
    pub fn global_defs(&self) -> Table<'a, Def> {
        self.table(&self.extents.global_defs)
    }

    /// The field definitions (PROGS-9).
    #[must_use]
    pub fn field_defs(&self) -> Table<'a, Def> {
        self.table(&self.extents.field_defs)
    }

    /// The functions (PROGS-10).
    #[must_use]
    pub fn functions(&self) -> Table<'a, Function> {
        self.table(&self.extents.functions)
    }

    /// The string table's bytes, padding included (PROGS-11).
    #[must_use]
    pub fn strings(&self) -> &'a [u8] {
        self.slice(&self.extents.strings)
    }

    /// The initial value of every global slot (PROGS-12).
    #[must_use]
    pub fn globals(&self) -> Table<'a, Slot> {
        self.table(&self.extents.globals)
    }

    /// FTE's line number of each statement, when the file has the table (PROGS-21.1).
    #[cfg(feature = "fte")]
    #[must_use]
    pub fn line_numbers(&self) -> Option<Table<'a, i32>> {
        self.extents
            .line_numbers
            .as_ref()
            .map(|range| self.table(range))
    }

    /// FTE's names of functions without bodies, in table order (PROGS-21.2).
    #[cfg(feature = "fte")]
    pub fn bodyless_functions(&self) -> impl Iterator<Item = &'a [u8]> + use<'a> {
        let names = self.slice(&self.extents.bodyless);
        names
            .strip_suffix(&[0])
            .into_iter()
            .flat_map(|names| names.split(|&byte| byte == 0))
    }

    /// The string at byte offset `offset` of the string table: the bytes up to the next NUL
    /// (PROGS-11.1, PROGS-11.3).
    ///
    /// Returns `None` for an offset outside the table or a string with no NUL before the end
    /// of the table. Run-time strings outside the table (PROGS-11.6) are the VM's to resolve.
    #[must_use]
    pub fn string(&self, offset: i32) -> Option<&'a [u8]> {
        let tail = self.strings().get(usize::try_from(offset).ok()?..)?;
        let end = tail.iter().position(|&byte| byte == 0)?;
        tail.get(..end)
    }

    /// The first global definition whose `ofs` is `ofs`, id's `ED_GlobalAtOfs` (PROGS-9.6).
    #[must_use]
    pub fn ed_global_at_ofs(&self, ofs: u32) -> Option<Def> {
        self.global_defs().iter().find(|def| def.ofs == ofs)
    }

    /// The first field definition whose `ofs` is `ofs`, id's `ED_FieldAtOfs` (PROGS-9.6).
    #[must_use]
    pub fn ed_field_at_ofs(&self, ofs: u32) -> Option<Def> {
        self.field_defs().iter().find(|def| def.ofs == ofs)
    }

    /// The first field definition named `name`, id's `ED_FindField` (PROGS-9.6).
    ///
    /// Names compare byte for byte, as `strcmp` does; `name` ends at its first NUL, if any.
    #[must_use]
    pub fn ed_find_field(&self, name: &[u8]) -> Option<Def> {
        let name = c_string(name);
        self.field_defs()
            .iter()
            .find(|def| self.string(def.s_name) == Some(name))
    }

    /// The first global definition named `name`, id's `ED_FindGlobal` (PROGS-9.6).
    ///
    /// Names compare byte for byte, as `strcmp` does; `name` ends at its first NUL, if any.
    #[must_use]
    pub fn ed_find_global(&self, name: &[u8]) -> Option<Def> {
        let name = c_string(name);
        self.global_defs()
            .iter()
            .find(|def| self.string(def.s_name) == Some(name))
    }

    /// The first function named `name`, id's `ED_FindFunction` (PROGS-9.6).
    ///
    /// Names compare byte for byte, as `strcmp` does; `name` ends at its first NUL, if any.
    #[must_use]
    pub fn ed_find_function(&self, name: &[u8]) -> Option<Function> {
        let name = c_string(name);
        self.functions()
            .iter()
            .find(|function| self.string(function.s_name) == Some(name))
    }

    /// Where the tables lie and the bytes between them, so writing the file back reproduces
    /// it (PROGS-22.1).
    ///
    /// `None` when two non-empty tables overlap, or one overlaps the header: such a file is
    /// written in qcc's layout instead (PROGS-22.2).
    #[must_use]
    pub fn layout(&self) -> Option<Layout> {
        let mut present: Vec<(TableId, Range<usize>)> = Vec::new();
        let mut empty: Vec<(TableId, u32)> = Vec::new();
        for (table, range) in self.table_extents() {
            if range.is_empty() {
                empty.push((table, u32::try_from(range.start).ok()?));
            } else {
                present.push((table, range));
            }
        }
        present.sort_by_key(|(table, range)| (range.start, *table));
        let header_end = self.format.header_size();
        let mut position = header_end;
        let mut lead = None;
        let mut tables: Vec<LayoutTable> = Vec::new();
        for (table, range) in present {
            if range.start < position {
                return None;
            }
            let gap = self.bytes.get(position..range.start)?.to_vec();
            match tables.last_mut() {
                Some(previous) => previous.gap_after = gap,
                None => lead = Some(gap),
            }
            tables.push(LayoutTable {
                table,
                gap_after: Vec::new(),
            });
            position = range.end;
        }
        let tail = self.bytes.get(position..)?.to_vec();
        match tables.last_mut() {
            Some(previous) => previous.gap_after = tail,
            None => lead = Some(tail),
        }
        Some(Layout {
            lead: lead.unwrap_or_default(),
            tables,
            empty,
        })
    }

    /// Every table this file has, with where it lies.
    fn table_extents(&self) -> Vec<(TableId, Range<usize>)> {
        let extents = &self.extents;
        #[cfg_attr(
            not(feature = "fte"),
            expect(unused_mut, reason = "FTE tables are added")
        )]
        let mut tables = vec![
            (TableId::Statements, extents.statements.clone()),
            (TableId::GlobalDefs, extents.global_defs.clone()),
            (TableId::FieldDefs, extents.field_defs.clone()),
            (TableId::Functions, extents.functions.clone()),
            (TableId::Strings, extents.strings.clone()),
            (TableId::Globals, extents.globals.clone()),
        ];
        #[cfg(feature = "fte")]
        if self.fte_header.is_some() {
            if let Some(range) = &extents.line_numbers {
                tables.push((TableId::LineNumbers, range.clone()));
            }
            tables.push((TableId::BodylessFunctions, extents.bodyless.clone()));
        }
        tables
    }
}

/// `name` up to its first NUL, as a C string argument would be.
fn c_string(name: &[u8]) -> &[u8] {
    name.split(|&byte| byte == 0).next().unwrap_or_default()
}

/// PROGS-15.2: the bytes `count * size` from `offset`, if they are inside the file.
fn extent(
    bytes: &[u8],
    table: TableId,
    offset: i32,
    count: i32,
    size: usize,
) -> Result<Range<usize>, ReadError> {
    let out_of_bounds = ReadError::TableOutOfBounds { table };
    let start = usize::try_from(offset).map_err(|_| out_of_bounds)?;
    let count = usize::try_from(count).map_err(|_| out_of_bounds)?;
    let end = count
        .checked_mul(size)
        .and_then(|len| start.checked_add(len))
        .ok_or(out_of_bounds)?;
    if end > bytes.len() {
        return Err(out_of_bounds);
    }
    Ok(start..end)
}

/// PROGS-19.2 to PROGS-19.3 and PROGS-21.4: the version 7 header and its layout.
#[cfg(feature = "fte")]
fn fte_layout(bytes: &[u8], format: Format) -> Result<(Format, Option<FteHeader>), ReadError> {
    if format == Format::V6 {
        return Ok((format, None));
    }
    let fte = FteHeader::decode(bytes).ok_or(ReadError::TooShort {
        len: bytes.len(),
        needed: PROGS_FTE_HEADER_SIZE,
    })?;
    let format = match fte.secondaryversion {
        PROGS_FTE_SECONDARY16 => Format::Fte16,
        PROGS_FTE_SECONDARY32 => Format::Fte32,
        secondaryversion => return Err(ReadError::UnsupportedFteLayout { secondaryversion }),
    };
    if fte.blockscompressed != 0 {
        return Err(ReadError::Compressed {
            blockscompressed: fte.blockscompressed,
        });
    }
    Ok((format, Some(fte)))
}

/// PROGS-21.1: the line number table, present when `ofslinenums` is not 0.
#[cfg(feature = "fte")]
fn fte_line_numbers(
    bytes: &[u8],
    fte: Option<&FteHeader>,
    numstatements: i32,
) -> Result<Option<Range<usize>>, ReadError> {
    let Some(fte) = fte.filter(|fte| fte.ofslinenums != 0) else {
        return Ok(None);
    };
    let offset = i32::try_from(fte.ofslinenums).unwrap_or(-1);
    extent(
        bytes,
        TableId::LineNumbers,
        offset,
        numstatements,
        PROGS_FTE_LINE_SIZE,
    )
    .map(Some)
}

/// PROGS-21.2: `numbodylessfuncs` NUL-terminated names from `ofsbodylessfuncs`.
#[cfg(feature = "fte")]
fn fte_bodyless(bytes: &[u8], fte: Option<&FteHeader>) -> Result<Range<usize>, ReadError> {
    let Some(fte) = fte else {
        return Ok(0..0);
    };
    let out_of_bounds = ReadError::TableOutOfBounds {
        table: TableId::BodylessFunctions,
    };
    let start = usize::try_from(fte.ofsbodylessfuncs).map_err(|_| out_of_bounds)?;
    let mut end = start;
    for _ in 0..fte.numbodylessfuncs {
        let tail = bytes.get(end..).ok_or(out_of_bounds)?;
        let len = tail
            .iter()
            .position(|&byte| byte == 0)
            .ok_or(out_of_bounds)?;
        end = end
            .checked_add(len)
            .and_then(|end| end.checked_add(1))
            .ok_or(out_of_bounds)?;
    }
    if end > bytes.len() {
        return Err(out_of_bounds);
    }
    Ok(start..end)
}
