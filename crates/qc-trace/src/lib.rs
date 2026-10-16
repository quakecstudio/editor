// SPDX-License-Identifier: GPL-3.0-or-later

//! The shared VM trace format: what happened in each QuakeC statement a VM executed.
//!
//! `qc-vm`'s trace hook and the id VM oracle each write this format independently, and the
//! differential runner compares the two traces statement by statement. This crate holds the
//! trace's data model, the compact binary form ([`encode`], [`decode`]) and the JSON debug form
//! ([`to_json`], [`from_json`]), and the rules for reading engine-specific values. The format
//! is `docs/spec/trace.md` (TRACE-1 to TRACE-7); it is a format of this project, not a port.

mod binary;
mod json;

#[cfg(test)]
mod tests;

use std::fmt;

pub use binary::{decode, encode, encode_header, encode_record};
pub use json::{from_json, header_to_json, record_to_json, to_json};

/// The trace format version.
///
/// `TRACE_VERSION` in TRACE-2.
pub const TRACE_VERSION: u32 = 1;

/// The four bytes a binary trace starts with: `QCTR`.
///
/// `TRACE_MAGIC` in TRACE-5.
pub const TRACE_MAGIC: [u8; 4] = *b"QCTR";

/// Bytes in a binary trace's header: the magic and six words.
///
/// `TRACE_HEADER_SIZE` in TRACE-5.
pub const TRACE_HEADER_SIZE: usize = 28;

/// Most bytes a uvar may take: 32 bits in groups of 7.
///
/// `TRACE_UVAR_MAX_BYTES` in TRACE-5.
pub const TRACE_UVAR_MAX_BYTES: usize = 5;

/// Tag byte of a run record (TRACE-5.6).
pub const TRACE_TAG_RUN: u8 = 0x01;
/// Tag byte of a step record (TRACE-5.6).
pub const TRACE_TAG_STEP: u8 = 0x02;
/// Tag byte of an exit record (TRACE-5.6).
pub const TRACE_TAG_EXIT: u8 = 0x03;
/// Tag byte of an error record (TRACE-5.6).
pub const TRACE_TAG_ERROR: u8 = 0x04;
/// Tag byte of a string record (TRACE-5.6).
pub const TRACE_TAG_STRING: u8 = 0x05;

/// Bytes per field slot, used to turn a field index into a pointer offset (TRACE-7.2).
const FIELD_SLOT_BYTES: u64 = 4;

/// Why a trace could not be built or decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TraceError {
    /// The header's numbers break TRACE-2.3.
    BadHeader,
    /// The version is not [`TRACE_VERSION`] (TRACE-2.2).
    UnsupportedVersion(u32),
    /// The binary stream does not start with [`TRACE_MAGIC`] (TRACE-5.7).
    BadMagic,
    /// The binary stream ends inside the header or a record; the offset is where the
    /// unfinished item starts (TRACE-5.7).
    Truncated {
        /// Byte offset of the header or record that is cut short.
        offset: usize,
    },
    /// A uvar is longer than 5 bytes, does not fit in 32 bits or is not minimal (TRACE-5.2).
    BadUvar {
        /// Byte offset of the uvar.
        offset: usize,
    },
    /// A record starts with an unknown tag byte (TRACE-5.7).
    UnknownTag {
        /// Byte offset of the tag.
        offset: usize,
        /// The tag byte.
        tag: u8,
    },
    /// A step's builtin flag byte is neither 0x00 nor 0x01 (TRACE-5.7).
    BadBuiltinFlag {
        /// Byte offset of the flag.
        offset: usize,
    },
    /// An opcode is above 65535 (TRACE-5.7).
    OpcodeOutOfRange(u32),
    /// A change set lists a slot twice (TRACE-3.2).
    DuplicateChange,
    /// A decoded change set is not in the order of TRACE-3.3.
    ChangesOutOfOrder,
    /// Text contains a NUL byte (TRACE-5.3, TRACE-4.8).
    NulInText,
    /// Text, or one list of a change set, has more than `u32::MAX` items, which a uvar count
    /// cannot hold (TRACE-5.3, TRACE-5.5).
    TooLong,
    /// A line of the JSON form breaks TRACE-6; `line` counts from 1.
    Json {
        /// The line number, from 1.
        line: usize,
        /// What is wrong with it.
        reason: String,
    },
}

impl fmt::Display for TraceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadHeader => write!(f, "trace header numbers are inconsistent (TRACE-2.3)"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported trace version {v}"),
            Self::BadMagic => write!(f, "not a binary trace: bad magic"),
            Self::Truncated { offset } => write!(f, "trace ends inside the item at byte {offset}"),
            Self::BadUvar { offset } => write!(f, "malformed uvar at byte {offset}"),
            Self::UnknownTag { offset, tag } => {
                write!(f, "unknown record tag 0x{tag:02x} at byte {offset}")
            }
            Self::BadBuiltinFlag { offset } => write!(f, "bad builtin flag at byte {offset}"),
            Self::OpcodeOutOfRange(op) => write!(f, "opcode {op} is above 65535"),
            Self::DuplicateChange => write!(f, "a change set lists a slot twice"),
            Self::ChangesOutOfOrder => write!(f, "a change set is out of order"),
            Self::NulInText => write!(f, "text contains a NUL byte"),
            Self::TooLong => write!(f, "text or change list is longer than a uvar can count"),
            Self::Json { line, reason } => write!(f, "JSON trace line {line}: {reason}"),
        }
    }
}

impl std::error::Error for TraceError {}

/// A trace's header: what a reader needs to interpret this emitter's values.
///
/// Implements TRACE-2. Built only through [`Header::new`], so every header satisfies TRACE-2.3.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Header {
    progs_crc: u16,
    edict_size: u32,
    field_offset: u32,
    entityfields: u32,
    numstrings: u32,
}

impl Header {
    /// A header for a VM whose entity values step by `edict_size` bytes and whose field slot 0
    /// is `field_offset` bytes after an entity value (TRACE-2.1).
    ///
    /// # Errors
    ///
    /// [`TraceError::BadHeader`] when `edict_size` is 0 or the `entityfields` 4-byte slots
    /// starting at `field_offset` do not fit inside `edict_size` (TRACE-2.3).
    pub fn new(
        progs_crc: u16,
        edict_size: u32,
        field_offset: u32,
        entityfields: u32,
        numstrings: u32,
    ) -> Result<Self, TraceError> {
        let fields_end = u64::from(entityfields)
            .checked_mul(FIELD_SLOT_BYTES)
            .and_then(|bytes| bytes.checked_add(u64::from(field_offset)));
        let fits = fields_end.is_some_and(|end| end <= u64::from(edict_size));
        if edict_size == 0 || !fits {
            return Err(TraceError::BadHeader);
        }
        Ok(Self {
            progs_crc,
            edict_size,
            field_offset,
            entityfields,
            numstrings,
        })
    }

    /// CRC of the whole progs.dat file (PROGS-6.1).
    #[must_use]
    pub const fn progs_crc(&self) -> u16 {
        self.progs_crc
    }

    /// Bytes between the entity values of consecutive edicts.
    #[must_use]
    pub const fn edict_size(&self) -> u32 {
        self.edict_size
    }

    /// Bytes from an entity value to the pointer of its field slot 0.
    #[must_use]
    pub const fn field_offset(&self) -> u32 {
        self.field_offset
    }

    /// Field slots per edict.
    #[must_use]
    pub const fn entityfields(&self) -> u32 {
        self.entityfields
    }

    /// Size of the string table in bytes.
    #[must_use]
    pub const fn numstrings(&self) -> u32 {
        self.numstrings
    }

    /// The edict an entity value names in this trace, if any.
    ///
    /// Implements TRACE-7.1: `bits`, read as a signed number, is `n * edict_size` exactly.
    #[must_use]
    pub fn entity_number(&self, bits: u32) -> Option<u32> {
        let value = bits.cast_signed();
        if value < 0 {
            return None;
        }
        let value = value.cast_unsigned();
        let size = self.edict_size;
        (value.checked_rem(size) == Some(0))
            .then(|| value.checked_div(size))
            .flatten()
    }

    /// The edict and field slot a pointer value names in this trace, if any.
    ///
    /// Implements TRACE-7.2: `bits` is `n * edict_size + field_offset + 4 * f` exactly, with
    /// `f` below `entityfields`.
    #[must_use]
    pub fn pointer_target(&self, bits: u32) -> Option<(u32, u32)> {
        let value = bits.cast_signed();
        if value < 0 {
            return None;
        }
        let value = value.cast_unsigned();
        let size = self.edict_size;
        let edict = value.checked_div(size)?;
        let within = value.checked_rem(size)?;
        let from_fields = within.checked_sub(self.field_offset)?;
        if from_fields.checked_rem(4)? != 0 {
            return None;
        }
        let field = from_fields.checked_div(4)?;
        (field < self.entityfields).then_some((edict, field))
    }
}

/// One global slot's new value in a change set (TRACE-3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GlobalChange {
    /// The global slot number.
    pub slot: u32,
    /// The slot's 32-bit pattern.
    pub bits: u32,
}

/// One entity field slot's new value in a change set (TRACE-3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FieldChange {
    /// The edict number; 0 is the world.
    pub edict: u32,
    /// The field slot number.
    pub field: u32,
    /// The slot's 32-bit pattern.
    pub bits: u32,
}

/// The globals and fields that changed since the previous change-carrying record.
///
/// Implements TRACE-3.2 and TRACE-3.3: each slot appears once, in ascending order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Changes {
    globals: Vec<GlobalChange>,
    fields: Vec<FieldChange>,
}

impl Changes {
    /// A change set from changes in any order; it sorts them as TRACE-3.3 says.
    ///
    /// # Errors
    ///
    /// [`TraceError::DuplicateChange`] when a global slot, or a field slot of one edict,
    /// appears twice (TRACE-3.2).
    pub fn new(
        mut globals: Vec<GlobalChange>,
        mut fields: Vec<FieldChange>,
    ) -> Result<Self, TraceError> {
        globals.sort_by_key(|g| g.slot);
        fields.sort_by_key(|f| (f.edict, f.field));
        let duplicate_global = globals
            .windows(2)
            .any(|w| w.first().map(|g| g.slot) == w.get(1).map(|g| g.slot));
        let duplicate_field = fields
            .windows(2)
            .any(|w| w.first().map(|f| (f.edict, f.field)) == w.get(1).map(|f| (f.edict, f.field)));
        if duplicate_global || duplicate_field {
            return Err(TraceError::DuplicateChange);
        }
        if u32::try_from(globals.len()).is_err() || u32::try_from(fields.len()).is_err() {
            return Err(TraceError::TooLong);
        }
        Ok(Self { globals, fields })
    }

    /// Builds a change set from changes that must already be in strictly ascending order,
    /// as a decoder reads them (TRACE-5.7).
    fn from_sorted(
        globals: Vec<GlobalChange>,
        fields: Vec<FieldChange>,
    ) -> Result<Self, TraceError> {
        let globals_ordered = globals.windows(2).all(|w| match (w.first(), w.get(1)) {
            (Some(a), Some(b)) => a.slot < b.slot,
            _ => true,
        });
        let fields_ordered = fields.windows(2).all(|w| match (w.first(), w.get(1)) {
            (Some(a), Some(b)) => (a.edict, a.field) < (b.edict, b.field),
            _ => true,
        });
        if globals_ordered && fields_ordered {
            Ok(Self { globals, fields })
        } else {
            Err(TraceError::ChangesOutOfOrder)
        }
    }

    /// The global changes, in ascending order of slot.
    #[must_use]
    pub fn globals(&self) -> &[GlobalChange] {
        &self.globals
    }

    /// The field changes, in ascending order of edict and then field slot.
    #[must_use]
    pub fn fields(&self) -> &[FieldChange] {
        &self.fields
    }

    /// Whether nothing changed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.globals.is_empty() && self.fields.is_empty()
    }
}

/// Bytes of an error message or an engine string, without a NUL (TRACE-4.8, TRACE-4.10).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Text(Vec<u8>);

impl Text {
    /// Text from bytes, which may be anything but NUL (PROGS-11.4).
    ///
    /// # Errors
    ///
    /// [`TraceError::NulInText`] when a byte is 0 (TRACE-5.3).
    pub fn new(bytes: impl Into<Vec<u8>>) -> Result<Self, TraceError> {
        let bytes = bytes.into();
        if bytes.contains(&0) {
            return Err(TraceError::NulInText);
        }
        if u32::try_from(bytes.len()).is_err() {
            return Err(TraceError::TooLong);
        }
        Ok(Self(bytes))
    }

    /// The bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// A statement that finished without an error (TRACE-4.2).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Step {
    /// Index of the function current when the statement became current.
    pub function: i32,
    /// The statement's index.
    pub statement: i32,
    /// The statement's opcode as stored (PROGS-7.1).
    pub opcode: u16,
    /// The builtin a `CALL` called, as `-first_statement` (TRACE-4.3).
    pub builtin: Option<u32>,
    /// What the statement changed (TRACE-3).
    pub changes: Changes,
}

/// One trace record (TRACE-4).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Record {
    /// `PR_ExecuteProgram` was called (TRACE-4.1).
    Run {
        /// The function index as passed.
        function: i32,
        /// The host's or builtin's changes since the previous record.
        changes: Changes,
    },
    /// A statement finished (TRACE-4.2).
    Step(Step),
    /// `PR_ExecuteProgram` returned to its caller (TRACE-4.5).
    Exit {
        /// Changes since the previous record.
        changes: Changes,
    },
    /// A runtime error ended the run (TRACE-4.6 to TRACE-4.8).
    Error {
        /// The current function, or the run's function for an error before its first
        /// statement.
        function: i32,
        /// The current statement, or -1 for an error before the run's first statement.
        statement: i32,
        /// The error message.
        message: Text,
        /// Changes since the previous record.
        changes: Changes,
    },
    /// A string value outside the string table now names this text (TRACE-4.10).
    String {
        /// The 32-bit string value.
        value: u32,
        /// The text, up to its NUL.
        text: Text,
    },
}

impl Record {
    /// The record's change set, or `None` for a string record, which carries none.
    #[must_use]
    pub fn changes(&self) -> Option<&Changes> {
        match self {
            Self::Run { changes, .. }
            | Self::Step(Step { changes, .. })
            | Self::Exit { changes }
            | Self::Error { changes, .. } => Some(changes),
            Self::String { .. } => None,
        }
    }
}

/// A whole trace: the header and every record, in order (TRACE-1.1).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Trace {
    /// The header.
    pub header: Header,
    /// The records, in the order their events happened.
    pub records: Vec<Record>,
}

/// What a string value names at one point in a trace (TRACE-7.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StringValue<'a> {
    /// An offset into the progs' string table, the same in every trace of one progs.
    Table(u32),
    /// An engine string, with the text of the latest string record for the value.
    Engine(&'a Text),
    /// A value outside the table that no earlier string record names.
    Unknown,
}

impl Trace {
    /// What the string value `bits` names just before record `before` (or after the last
    /// record when `before` is past the end).
    ///
    /// Implements TRACE-7.3.
    #[must_use]
    pub fn string_value(&self, before: usize, bits: u32) -> StringValue<'_> {
        let value = bits.cast_signed();
        if value >= 0 && value.cast_unsigned() < self.header.numstrings {
            return StringValue::Table(value.cast_unsigned());
        }
        self.records
            .iter()
            .take(before)
            .rev()
            .find_map(|record| match record {
                Record::String { value, text } if *value == bits => Some(StringValue::Engine(text)),
                _ => None,
            })
            .unwrap_or(StringValue::Unknown)
    }
}
