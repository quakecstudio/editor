// SPDX-License-Identifier: GPL-3.0-or-later

//! The binary form of a trace (TRACE-5).

use crate::{
    Changes, FieldChange, GlobalChange, Header, Record, Step, TRACE_HEADER_SIZE, TRACE_MAGIC,
    TRACE_TAG_ERROR, TRACE_TAG_EXIT, TRACE_TAG_RUN, TRACE_TAG_STEP, TRACE_TAG_STRING,
    TRACE_UVAR_MAX_BYTES, TRACE_VERSION, Text, Trace, TraceError,
};

/// Builtin flag byte of a step without a builtin (TRACE-5.6).
const NO_BUILTIN: u8 = 0x00;
/// Builtin flag byte of a step with a builtin (TRACE-5.6).
const HAS_BUILTIN: u8 = 0x01;

/// Encodes a whole trace in the binary form.
///
/// Implements TRACE-5: the header, then each record.
#[must_use]
pub fn encode(trace: &Trace) -> Vec<u8> {
    let mut out = Vec::new();
    encode_header(&trace.header, &mut out);
    for record in &trace.records {
        encode_record(record, &mut out);
    }
    out
}

/// Appends a binary header to `out` (TRACE-5.4), for an emitter that streams its trace.
pub fn encode_header(header: &Header, out: &mut Vec<u8>) {
    out.extend_from_slice(&TRACE_MAGIC);
    for word in [
        TRACE_VERSION,
        u32::from(header.progs_crc),
        header.edict_size,
        header.field_offset,
        header.entityfields,
        header.numstrings,
    ] {
        put_word(out, word);
    }
}

/// Appends one binary record to `out` (TRACE-5.6), for an emitter that streams its trace.
pub fn encode_record(record: &Record, out: &mut Vec<u8>) {
    match record {
        Record::Run { function, changes } => {
            out.push(TRACE_TAG_RUN);
            put_svar(out, *function);
            put_changes(out, changes);
        }
        Record::Step(Step {
            function,
            statement,
            opcode,
            builtin,
            changes,
        }) => {
            out.push(TRACE_TAG_STEP);
            put_svar(out, *function);
            put_svar(out, *statement);
            put_uvar(out, u32::from(*opcode));
            match builtin {
                None => out.push(NO_BUILTIN),
                Some(number) => {
                    out.push(HAS_BUILTIN);
                    put_uvar(out, *number);
                }
            }
            put_changes(out, changes);
        }
        Record::Exit { changes } => {
            out.push(TRACE_TAG_EXIT);
            put_changes(out, changes);
        }
        Record::Error {
            function,
            statement,
            message,
            changes,
        } => {
            out.push(TRACE_TAG_ERROR);
            put_svar(out, *function);
            put_svar(out, *statement);
            put_text(out, message);
            put_changes(out, changes);
        }
        Record::String { value, text } => {
            out.push(TRACE_TAG_STRING);
            put_word(out, *value);
            put_text(out, text);
        }
    }
}

/// Decodes a whole binary trace.
///
/// Implements TRACE-5 and the reader's checks of TRACE-2.2, TRACE-2.3, TRACE-5.2 and
/// TRACE-5.7.
///
/// # Errors
///
/// The first [`TraceError`] found, with the byte offset where it applies.
pub fn decode(bytes: &[u8]) -> Result<Trace, TraceError> {
    let mut reader = Reader { bytes, pos: 0 };
    let header = reader.header()?;
    let mut records = Vec::new();
    while reader.pos < bytes.len() {
        records.push(reader.record()?);
    }
    Ok(Trace { header, records })
}

fn put_word(out: &mut Vec<u8>, word: u32) {
    out.extend_from_slice(&word.to_le_bytes());
}

/// TRACE-5.1: LEB128, least significant group first.
fn put_uvar(out: &mut Vec<u8>, mut value: u32) {
    loop {
        let low = (value & 0x7F).to_le_bytes();
        let [group, ..] = low;
        value = value.wrapping_shr(7);
        if value == 0 {
            out.push(group);
            return;
        }
        out.push(group | 0x80);
    }
}

/// TRACE-5.1: zigzag, `(n << 1) ^ (n >> 31)` with an arithmetic shift.
fn zigzag(n: i32) -> u32 {
    (n.wrapping_shl(1) ^ n.wrapping_shr(31)).cast_unsigned()
}

fn unzigzag(u: u32) -> i32 {
    u.wrapping_shr(1).cast_signed() ^ (u & 1).cast_signed().wrapping_neg()
}

fn put_svar(out: &mut Vec<u8>, n: i32) {
    put_uvar(out, zigzag(n));
}

fn put_text(out: &mut Vec<u8>, text: &Text) {
    put_length(out, text.as_bytes().len());
    out.extend_from_slice(text.as_bytes());
}

/// A count or length as a uvar. [`Text::new`] and [`Changes::new`] reject anything longer
/// than `u32::MAX`, so the fallback is never taken.
fn put_length(out: &mut Vec<u8>, len: usize) {
    put_uvar(out, u32::try_from(len).unwrap_or(u32::MAX));
}

fn put_changes(out: &mut Vec<u8>, changes: &Changes) {
    put_length(out, changes.globals.len());
    for change in &changes.globals {
        put_uvar(out, change.slot);
        put_word(out, change.bits);
    }
    put_length(out, changes.fields.len());
    for change in &changes.fields {
        put_uvar(out, change.edict);
        put_uvar(out, change.field);
        put_word(out, change.bits);
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn header(&mut self) -> Result<Header, TraceError> {
        let Some(head) = self.bytes.get(..TRACE_HEADER_SIZE) else {
            return Err(
                if self.bytes.starts_with(&TRACE_MAGIC) || self.bytes.len() < 4 {
                    TraceError::Truncated { offset: 0 }
                } else {
                    TraceError::BadMagic
                },
            );
        };
        if head.get(..4) != Some(TRACE_MAGIC.as_slice()) {
            return Err(TraceError::BadMagic);
        }
        self.pos = 4;
        let version = self.word(0)?;
        if version != TRACE_VERSION {
            return Err(TraceError::UnsupportedVersion(version));
        }
        let crc = self.word(0)?;
        let edict_size = self.word(0)?;
        let field_offset = self.word(0)?;
        let entityfields = self.word(0)?;
        let numstrings = self.word(0)?;
        let crc = u16::try_from(crc).map_err(|_| TraceError::BadHeader)?;
        Header::new(crc, edict_size, field_offset, entityfields, numstrings)
    }

    fn record(&mut self) -> Result<Record, TraceError> {
        let start = self.pos;
        let tag = self.byte(start)?;
        match tag {
            TRACE_TAG_RUN => {
                let function = self.svar(start)?;
                let changes = self.changes(start)?;
                Ok(Record::Run { function, changes })
            }
            TRACE_TAG_STEP => {
                let function = self.svar(start)?;
                let statement = self.svar(start)?;
                let opcode = self.uvar(start)?;
                let opcode =
                    u16::try_from(opcode).map_err(|_| TraceError::OpcodeOutOfRange(opcode))?;
                let flag_at = self.pos;
                let builtin = match self.byte(start)? {
                    NO_BUILTIN => None,
                    HAS_BUILTIN => Some(self.uvar(start)?),
                    _ => return Err(TraceError::BadBuiltinFlag { offset: flag_at }),
                };
                let changes = self.changes(start)?;
                Ok(Record::Step(Step {
                    function,
                    statement,
                    opcode,
                    builtin,
                    changes,
                }))
            }
            TRACE_TAG_EXIT => Ok(Record::Exit {
                changes: self.changes(start)?,
            }),
            TRACE_TAG_ERROR => {
                let function = self.svar(start)?;
                let statement = self.svar(start)?;
                let message = self.text(start)?;
                let changes = self.changes(start)?;
                Ok(Record::Error {
                    function,
                    statement,
                    message,
                    changes,
                })
            }
            TRACE_TAG_STRING => {
                let value = self.word(start)?;
                let text = self.text(start)?;
                Ok(Record::String { value, text })
            }
            _ => Err(TraceError::UnknownTag { offset: start, tag }),
        }
    }

    /// Takes `n` bytes; `start` is the record (or header) to blame if they are missing.
    fn take(&mut self, n: usize, start: usize) -> Result<&[u8], TraceError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or(TraceError::Truncated { offset: start })?;
        let slice = self
            .bytes
            .get(self.pos..end)
            .ok_or(TraceError::Truncated { offset: start })?;
        self.pos = end;
        Ok(slice)
    }

    fn byte(&mut self, start: usize) -> Result<u8, TraceError> {
        let [b] = self.take(1, start)? else {
            return Err(TraceError::Truncated { offset: start });
        };
        Ok(*b)
    }

    fn word(&mut self, start: usize) -> Result<u32, TraceError> {
        let bytes: [u8; 4] = self
            .take(4, start)?
            .try_into()
            .map_err(|_| TraceError::Truncated { offset: start })?;
        Ok(u32::from_le_bytes(bytes))
    }

    /// TRACE-5.2: at most 5 bytes, fits in 32 bits, minimal.
    fn uvar(&mut self, start: usize) -> Result<u32, TraceError> {
        let at = self.pos;
        let mut value: u32 = 0;
        for index in 0..TRACE_UVAR_MAX_BYTES {
            let b = self.byte(start)?;
            let group = u32::from(b & 0x7F);
            let last = b & 0x80 == 0;
            let is_fifth = index.checked_add(1) == Some(TRACE_UVAR_MAX_BYTES);
            if is_fifth && b > 0x0F {
                return Err(TraceError::BadUvar { offset: at });
            }
            let shift = u32::try_from(index).map_err(|_| TraceError::BadUvar { offset: at })?;
            value |= group.wrapping_shl(shift.wrapping_mul(7));
            if last {
                if index > 0 && b == 0 {
                    return Err(TraceError::BadUvar { offset: at });
                }
                return Ok(value);
            }
        }
        Err(TraceError::BadUvar { offset: at })
    }

    fn svar(&mut self, start: usize) -> Result<i32, TraceError> {
        Ok(unzigzag(self.uvar(start)?))
    }

    fn length(&mut self, start: usize) -> Result<usize, TraceError> {
        let at = self.pos;
        let n = self.uvar(start)?;
        usize::try_from(n).map_err(|_| TraceError::BadUvar { offset: at })
    }

    fn text(&mut self, start: usize) -> Result<Text, TraceError> {
        let len = self.length(start)?;
        Text::new(self.take(len, start)?)
    }

    fn changes(&mut self, start: usize) -> Result<Changes, TraceError> {
        // A change takes at least 5 bytes, so a count larger than the rest of the stream is
        // truncated; checking first keeps a corrupt count from reserving memory.
        let globals_len = self.count(start)?;
        let mut globals = Vec::with_capacity(globals_len);
        for _ in 0..globals_len {
            let slot = self.uvar(start)?;
            let bits = self.word(start)?;
            globals.push(GlobalChange { slot, bits });
        }
        let fields_len = self.count(start)?;
        let mut fields = Vec::with_capacity(fields_len);
        for _ in 0..fields_len {
            let edict = self.uvar(start)?;
            let field = self.uvar(start)?;
            let bits = self.word(start)?;
            fields.push(FieldChange { edict, field, bits });
        }
        Changes::from_sorted(globals, fields)
    }

    fn count(&mut self, start: usize) -> Result<usize, TraceError> {
        let n = self.length(start)?;
        let remaining = self.bytes.len().saturating_sub(self.pos);
        if n > remaining {
            return Err(TraceError::Truncated { offset: start });
        }
        Ok(n)
    }
}
