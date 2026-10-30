// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_comp.h, dprograms_t, dstatement_t, ddef_t, dfunction_t

//! The header and the records of each progs.dat table, and how they decode from bytes.
//!
//! Implements PROGS-2, PROGS-7, PROGS-9 and PROGS-10 of `docs/spec/progs.md`, and the
//! version 7 layouts of PROGS-19 and PROGS-20.

use qc_types::{DefType, Slot};

/// The format version id's engine loads, `PROG_VERSION`.
///
/// `PROGS_VERSION` in PROGS-3.
pub const PROGS_VERSION: i32 = 6;

/// The header CRC of id's `progdefs.h`, `PROGHEADER_CRC`.
///
/// `PROGS_HEADER_CRC` in PROGS-3.
pub const PROGS_HEADER_CRC: i32 = 5927;

/// Bytes in a version 6 header, `sizeof(dprograms_t)`.
///
/// `PROGS_HEADER_SIZE` in PROGS-2.
pub const PROGS_HEADER_SIZE: usize = 60;

/// Bytes in a version 6 statement, `sizeof(dstatement_t)`.
///
/// `PROGS_STATEMENT_SIZE` in PROGS-2.
pub const PROGS_STATEMENT_SIZE: usize = 8;

/// Bytes in a version 6 definition, `sizeof(ddef_t)`.
///
/// `PROGS_DEF_SIZE` in PROGS-2.
pub const PROGS_DEF_SIZE: usize = 8;

/// Bytes in a function, `sizeof(dfunction_t)`.
///
/// `PROGS_FUNCTION_SIZE` in PROGS-2.
pub const PROGS_FUNCTION_SIZE: usize = 36;

/// Bytes in a global slot.
///
/// `PROGS_GLOBAL_SIZE` in PROGS-2.
pub const PROGS_GLOBAL_SIZE: usize = 4;

/// Alignment qcc pads the string table to.
///
/// `PROGS_STRING_ALIGN` in PROGS-1, from `WriteData` in `qcc/qcc.c`.
pub const PROGS_STRING_ALIGN: usize = 4;

/// Bit 15 of a definition's type: the global is stored in saved games, id's `DEF_SAVEGLOBAL`.
///
/// `PROGS_DEF_SAVEGLOBAL` in PROGS-9.
pub const PROGS_DEF_SAVEGLOBAL: u32 = 0x8000;

/// FTEQCC's extended format version, `PROG_EXTENDEDVERSION`.
///
/// `PROGS_VERSION_FTE` in PROGS-19.
pub const PROGS_VERSION_FTE: i32 = 7;

/// Bytes in a version 7 header.
///
/// `PROGS_FTE_HEADER_SIZE` in PROGS-19.
pub const PROGS_FTE_HEADER_SIZE: usize = 92;

/// `secondaryversion` of the 16-bit version 7 layout, `PROG_SECONDARYVERSION16`.
///
/// `PROGS_FTE_SECONDARY16` in PROGS-19.
pub const PROGS_FTE_SECONDARY16: i32 = 0x021B_1461;

/// `secondaryversion` of the 32-bit version 7 layout, `PROG_SECONDARYVERSION32`.
///
/// `PROGS_FTE_SECONDARY32` in PROGS-19.
pub const PROGS_FTE_SECONDARY32: i32 = 0x6516_7402;

/// Bytes in a statement of the 32-bit version 7 layout, `sizeof(dstatement32_t)`.
///
/// `PROGS_FTE32_STATEMENT_SIZE` in PROGS-20.
pub const PROGS_FTE32_STATEMENT_SIZE: usize = 16;

/// Bytes in a definition of the 32-bit version 7 layout, `sizeof(ddef32_t)`.
///
/// `PROGS_FTE32_DEF_SIZE` in PROGS-20.
pub const PROGS_FTE32_DEF_SIZE: usize = 12;

/// Bytes in one record of FTE's type table, `sizeof(typeinfo_t)` (PROGS-21.8).
pub const PROGS_FTE_TYPE_SIZE: usize = 28;

/// Bytes in one line number (PROGS-21.1).
pub const PROGS_FTE_LINE_SIZE: usize = 4;

/// Which layout a file uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    /// Version 6, the format id's engine loads (PROGS-2 to PROGS-13).
    V6,
    /// FTE version 7 with the 16-bit statements and definitions of version 6 (PROGS-19.2).
    Fte16,
    /// FTE version 7 with 32-bit statements and definitions (PROGS-20).
    Fte32,
}

impl Format {
    /// The `version` the header stores.
    #[must_use]
    pub const fn version(self) -> i32 {
        match self {
            Self::V6 => PROGS_VERSION,
            Self::Fte16 | Self::Fte32 => PROGS_VERSION_FTE,
        }
    }

    /// The `secondaryversion` a version 7 header gives this layout (PROGS-19.2); `None` for
    /// version 6, which has none.
    #[must_use]
    pub const fn secondary_version(self) -> Option<i32> {
        match self {
            Self::V6 => None,
            Self::Fte16 => Some(PROGS_FTE_SECONDARY16),
            Self::Fte32 => Some(PROGS_FTE_SECONDARY32),
        }
    }

    /// Bytes in the header.
    #[must_use]
    pub const fn header_size(self) -> usize {
        match self {
            Self::V6 => PROGS_HEADER_SIZE,
            Self::Fte16 | Self::Fte32 => PROGS_FTE_HEADER_SIZE,
        }
    }

    /// Bytes in one statement.
    #[must_use]
    pub const fn statement_size(self) -> usize {
        match self {
            Self::V6 => PROGS_STATEMENT_SIZE,
            Self::Fte16 => PROGS_STATEMENT_SIZE,
            Self::Fte32 => PROGS_FTE32_STATEMENT_SIZE,
        }
    }

    /// Bytes in one global or field definition.
    #[must_use]
    pub const fn def_size(self) -> usize {
        match self {
            Self::V6 => PROGS_DEF_SIZE,
            Self::Fte16 => PROGS_DEF_SIZE,
            Self::Fte32 => PROGS_FTE32_DEF_SIZE,
        }
    }
}

/// The 15 fields of the version 6 header, id's `dprograms_t`, as stored.
///
/// Implements PROGS-2.1. Offsets are from the start of the file (PROGS-2.2); the fields are
/// signed, as in id's struct (PROGS-2.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Header {
    /// Format version: 6, or 7 for FTE (PROGS-3, PROGS-19).
    pub version: i32,
    /// CRC of the `progdefs.h` qcc generated (PROGS-5).
    pub crc: i32,
    /// File offset of the statements.
    pub ofs_statements: i32,
    /// Number of statements.
    pub numstatements: i32,
    /// File offset of the global definitions.
    pub ofs_globaldefs: i32,
    /// Number of global definitions.
    pub numglobaldefs: i32,
    /// File offset of the field definitions.
    pub ofs_fielddefs: i32,
    /// Number of field definitions.
    pub numfielddefs: i32,
    /// File offset of the functions.
    pub ofs_functions: i32,
    /// Number of functions.
    pub numfunctions: i32,
    /// File offset of the string table.
    pub ofs_strings: i32,
    /// Size of the string table in bytes.
    pub numstrings: i32,
    /// File offset of the globals.
    pub ofs_globals: i32,
    /// Number of global slots.
    pub numglobals: i32,
    /// Number of field slots in every entity (PROGS-13).
    pub entityfields: i32,
}

impl Header {
    /// Decodes the first [`PROGS_HEADER_SIZE`] bytes, little-endian (PROGS-1.1).
    pub(crate) fn decode(bytes: &[u8]) -> Option<Self> {
        let field = |index: usize| read_i32(bytes, index.checked_mul(4)?);
        Some(Self {
            version: field(0)?,
            crc: field(1)?,
            ofs_statements: field(2)?,
            numstatements: field(3)?,
            ofs_globaldefs: field(4)?,
            numglobaldefs: field(5)?,
            ofs_fielddefs: field(6)?,
            numfielddefs: field(7)?,
            ofs_functions: field(8)?,
            numfunctions: field(9)?,
            ofs_strings: field(10)?,
            numstrings: field(11)?,
            ofs_globals: field(12)?,
            numglobals: field(13)?,
            entityfields: field(14)?,
        })
    }

    /// The 15 fields in file order.
    pub(crate) const fn fields(&self) -> [i32; 15] {
        [
            self.version,
            self.crc,
            self.ofs_statements,
            self.numstatements,
            self.ofs_globaldefs,
            self.numglobaldefs,
            self.ofs_fielddefs,
            self.numfielddefs,
            self.ofs_functions,
            self.numfunctions,
            self.ofs_strings,
            self.numstrings,
            self.ofs_globals,
            self.numglobals,
            self.entityfields,
        ]
    }
}

/// The eight fields a version 7 header adds after the version 6 fields, as stored.
///
/// Implements PROGS-19.1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FteHeader {
    /// Offset of the embedded source files, 0 if none (PROGS-21.7).
    pub ofsfiles: u32,
    /// Offset of the line number table, 0 if none (PROGS-21.1).
    pub ofslinenums: u32,
    /// Offset of the names of functions without bodies (PROGS-21.2).
    pub ofsbodylessfuncs: u32,
    /// Number of those names.
    pub numbodylessfuncs: u32,
    /// Offset of the type table, 0 if none (PROGS-21.8).
    pub ofs_types: u32,
    /// Number of type records.
    pub numtypes: u32,
    /// Bit mask of compressed tables (PROGS-21.3).
    pub blockscompressed: u32,
    /// Which version 7 layout (PROGS-19.2).
    pub secondaryversion: i32,
}

impl FteHeader {
    /// Decodes the eight fields from the bytes after the version 6 header.
    pub(crate) fn decode(bytes: &[u8]) -> Option<Self> {
        let field =
            |index: usize| read_u32(bytes, PROGS_HEADER_SIZE.checked_add(index.checked_mul(4)?)?);
        Some(Self {
            ofsfiles: field(0)?,
            ofslinenums: field(1)?,
            ofsbodylessfuncs: field(2)?,
            numbodylessfuncs: field(3)?,
            ofs_types: field(4)?,
            numtypes: field(5)?,
            blockscompressed: field(6)?,
            secondaryversion: read_i32(bytes, PROGS_HEADER_SIZE.checked_add(28)?)?,
        })
    }
}

/// One statement, id's `dstatement_t`.
///
/// Implements PROGS-7.1. The fields are wide enough for every layout: a version 6 operand is
/// a signed 16-bit value, sign-extended (PROGS-7.2); a 16-bit version 7 operand is unsigned,
/// zero-extended (PROGS-19.5); a 32-bit version 7 operand is the stored 32 bits read as a
/// two's-complement `i32`, which FTE's VM also does for jumps (PROGS-20.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Statement {
    /// The opcode (PROGS-8); not checked at load (PROGS-7.4).
    pub op: u32,
    /// First operand.
    pub a: i32,
    /// Second operand.
    pub b: i32,
    /// Third operand.
    pub c: i32,
}

/// One global or field definition, id's `ddef_t`.
///
/// Implements PROGS-9.1 and, for the 32-bit version 7 layout, PROGS-20.2.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Def {
    /// The stored type field: bits 0 to 14 are the type, bit 15 is `DEF_SAVEGLOBAL`.
    pub type_bits: u32,
    /// The global or field slot index, unsigned (PROGS-9.2).
    pub ofs: u32,
    /// Byte offset of the name in the string table (PROGS-11.1).
    pub s_name: i32,
}

impl Def {
    /// Whether `DEF_SAVEGLOBAL` is set (PROGS-9.3).
    #[must_use]
    pub const fn is_save_global(&self) -> bool {
        self.type_bits & PROGS_DEF_SAVEGLOBAL != 0
    }

    /// The type with `DEF_SAVEGLOBAL` removed: bits 0 to 14 (PROGS-9.1, LIMIT-1.3). A number
    /// above 7 is kept, not rejected (PROGS-16.3).
    #[must_use]
    pub const fn def_type(&self) -> DefType {
        let [low, high, _, _] = self.type_bits.to_le_bytes();
        DefType::from_number(u16::from_le_bytes([low, high & 0x7F]))
    }
}

/// One function, id's `dfunction_t`.
///
/// Implements PROGS-10.1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Function {
    /// First statement, or minus the builtin number when negative (PROGS-10.2).
    pub first_statement: i32,
    /// First global slot of the parameters and locals.
    pub parm_start: i32,
    /// Slots from `parm_start` that a call saves and restores.
    pub locals: i32,
    /// Runtime statement counter, kept as stored (PROGS-10.6).
    pub profile: i32,
    /// Byte offset of the name in the string table.
    pub s_name: i32,
    /// Byte offset of the source file name in the string table.
    pub s_file: i32,
    /// Number of parameters; negative for a variadic builtin (PROGS-10.5).
    pub numparms: i32,
    /// Slots each parameter takes, single bytes not swapped (PROGS-10.4).
    pub parm_size: [u8; 8],
}

impl Function {
    /// The builtin number, when `first_statement` is negative (PROGS-10.2).
    ///
    /// Returns `None` for a QC function, and for -2147483648, which has no positive number
    /// (LIMIT-5.3).
    #[must_use]
    pub const fn builtin_number(&self) -> Option<i32> {
        if self.first_statement < 0 {
            self.first_statement.checked_neg()
        } else {
            None
        }
    }
}

/// A record of one of the tables, decoded from its bytes in a given [`Format`].
pub trait Record: Sized + sealed::Sealed {
    /// Bytes in one record in `format`.
    fn size(format: Format) -> usize;
    /// Decodes one record from exactly [`Record::size`] bytes.
    fn decode(bytes: &[u8], format: Format) -> Option<Self>;
    /// Appends the record's [`Record::size`] bytes in `format` to `out`; `None` when a field
    /// does not fit the format's width, leaving `out` as it was or partly written.
    fn encode(&self, out: &mut Vec<u8>, format: Format) -> Option<()>;
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::Statement {}
    impl Sealed for super::Def {}
    impl Sealed for super::Function {}
    impl Sealed for qc_types::Slot {}
    impl Sealed for i32 {}
}

impl Record for Statement {
    fn size(format: Format) -> usize {
        format.statement_size()
    }

    fn decode(bytes: &[u8], format: Format) -> Option<Self> {
        match format {
            Format::V6 => Some(Self {
                op: u32::from(read_u16(bytes, 0)?),
                a: i32::from(read_i16(bytes, 2)?),
                b: i32::from(read_i16(bytes, 4)?),
                c: i32::from(read_i16(bytes, 6)?),
            }),
            Format::Fte16 => Some(Self {
                op: u32::from(read_u16(bytes, 0)?),
                a: i32::from(read_u16(bytes, 2)?),
                b: i32::from(read_u16(bytes, 4)?),
                c: i32::from(read_u16(bytes, 6)?),
            }),
            Format::Fte32 => Some(Self {
                op: read_u32(bytes, 0)?,
                a: read_i32(bytes, 4)?,
                b: read_i32(bytes, 8)?,
                c: read_i32(bytes, 12)?,
            }),
        }
    }
    fn encode(&self, out: &mut Vec<u8>, format: Format) -> Option<()> {
        match format {
            Format::V6 => {
                out.extend(u16::try_from(self.op).ok()?.to_le_bytes());
                for operand in [self.a, self.b, self.c] {
                    out.extend(i16::try_from(operand).ok()?.to_le_bytes());
                }
            }
            Format::Fte16 => {
                out.extend(u16::try_from(self.op).ok()?.to_le_bytes());
                for operand in [self.a, self.b, self.c] {
                    out.extend(u16::try_from(operand).ok()?.to_le_bytes());
                }
            }
            Format::Fte32 => {
                out.extend(self.op.to_le_bytes());
                for operand in [self.a, self.b, self.c] {
                    out.extend(operand.to_le_bytes());
                }
            }
        }
        Some(())
    }
}

impl Record for Def {
    fn size(format: Format) -> usize {
        format.def_size()
    }

    fn decode(bytes: &[u8], format: Format) -> Option<Self> {
        match format {
            Format::V6 => Some(Self {
                type_bits: u32::from(read_u16(bytes, 0)?),
                ofs: u32::from(read_u16(bytes, 2)?),
                s_name: read_i32(bytes, 4)?,
            }),
            Format::Fte16 => Self::decode(bytes, Format::V6),
            Format::Fte32 => Some(Self {
                type_bits: read_u32(bytes, 0)?,
                ofs: read_u32(bytes, 4)?,
                s_name: read_i32(bytes, 8)?,
            }),
        }
    }
    fn encode(&self, out: &mut Vec<u8>, format: Format) -> Option<()> {
        match format {
            Format::V6 => {
                out.extend(u16::try_from(self.type_bits).ok()?.to_le_bytes());
                out.extend(u16::try_from(self.ofs).ok()?.to_le_bytes());
            }
            Format::Fte16 => return self.encode(out, Format::V6),
            Format::Fte32 => {
                out.extend(self.type_bits.to_le_bytes());
                out.extend(self.ofs.to_le_bytes());
            }
        }
        out.extend(self.s_name.to_le_bytes());
        Some(())
    }
}

impl Record for Function {
    fn size(_format: Format) -> usize {
        PROGS_FUNCTION_SIZE
    }

    fn decode(bytes: &[u8], _format: Format) -> Option<Self> {
        Some(Self {
            first_statement: read_i32(bytes, 0)?,
            parm_start: read_i32(bytes, 4)?,
            locals: read_i32(bytes, 8)?,
            profile: read_i32(bytes, 12)?,
            s_name: read_i32(bytes, 16)?,
            s_file: read_i32(bytes, 20)?,
            numparms: read_i32(bytes, 24)?,
            parm_size: bytes.get(28..36)?.try_into().ok()?,
        })
    }
    fn encode(&self, out: &mut Vec<u8>, _format: Format) -> Option<()> {
        for field in [
            self.first_statement,
            self.parm_start,
            self.locals,
            self.profile,
            self.s_name,
            self.s_file,
            self.numparms,
        ] {
            out.extend(field.to_le_bytes());
        }
        out.extend(self.parm_size);
        Some(())
    }
}

impl Record for Slot {
    fn size(_format: Format) -> usize {
        PROGS_GLOBAL_SIZE
    }

    fn decode(bytes: &[u8], _format: Format) -> Option<Self> {
        read_array(bytes, 0).map(Self::from_le_bytes)
    }

    fn encode(&self, out: &mut Vec<u8>, _format: Format) -> Option<()> {
        out.extend(self.to_le_bytes());
        Some(())
    }
}

/// A line number of FTE's line number table (PROGS-21.1).
impl Record for i32 {
    fn size(_format: Format) -> usize {
        4
    }

    fn decode(bytes: &[u8], _format: Format) -> Option<Self> {
        read_i32(bytes, 0)
    }

    fn encode(&self, out: &mut Vec<u8>, _format: Format) -> Option<()> {
        out.extend(self.to_le_bytes());
        Some(())
    }
}

fn read_array<const N: usize>(bytes: &[u8], at: usize) -> Option<[u8; N]> {
    bytes.get(at..at.checked_add(N)?)?.try_into().ok()
}

pub(crate) fn read_i32(bytes: &[u8], at: usize) -> Option<i32> {
    read_array(bytes, at).map(i32::from_le_bytes)
}

pub(crate) fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    read_array(bytes, at).map(u32::from_le_bytes)
}

fn read_u16(bytes: &[u8], at: usize) -> Option<u16> {
    read_array(bytes, at).map(u16::from_le_bytes)
}

fn read_i16(bytes: &[u8], at: usize) -> Option<i16> {
    read_array(bytes, at).map(i16::from_le_bytes)
}
