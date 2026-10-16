// SPDX-License-Identifier: GPL-3.0-or-later

//! The JSON debug form of a trace (TRACE-6).

use serde_json::{Map, Value};

use crate::{
    Changes, FieldChange, GlobalChange, Header, Record, Step, TRACE_VERSION, Text, Trace,
    TraceError,
};

/// The `trace` key's value in the header line (TRACE-6.2).
const FORMAT_NAME: &str = "qc-trace";

/// Encodes a whole trace in the JSON form: the header line, then one line per record.
///
/// Implements TRACE-6.1.
#[must_use]
pub fn to_json(trace: &Trace) -> String {
    let mut out = header_to_json(&trace.header);
    for record in &trace.records {
        out.push_str(&record_to_json(record));
    }
    out
}

/// The header's JSON line, ending in a line feed (TRACE-6.2, TRACE-6.6).
#[must_use]
pub fn header_to_json(header: &Header) -> String {
    format!(
        "{{\"trace\":\"{FORMAT_NAME}\",\"version\":{TRACE_VERSION},\"progs_crc\":{},\
         \"edict_size\":{},\"field_offset\":{},\"entityfields\":{},\"numstrings\":{}}}\n",
        header.progs_crc,
        header.edict_size,
        header.field_offset,
        header.entityfields,
        header.numstrings
    )
}

/// One record's JSON line, ending in a line feed (TRACE-6.3 to TRACE-6.6).
#[must_use]
pub fn record_to_json(record: &Record) -> String {
    let mut out = String::new();
    match record {
        Record::Run { function, changes } => {
            out.push_str(&format!("{{\"t\":\"run\",\"function\":{function}"));
            push_changes(&mut out, changes);
        }
        Record::Step(Step {
            function,
            statement,
            opcode,
            builtin,
            changes,
        }) => {
            out.push_str(&format!(
                "{{\"t\":\"step\",\"function\":{function},\"statement\":{statement},\
                 \"opcode\":{opcode}"
            ));
            if let Some(number) = builtin {
                out.push_str(&format!(",\"builtin\":{number}"));
            }
            push_changes(&mut out, changes);
        }
        Record::Exit { changes } => {
            out.push_str("{\"t\":\"exit\"");
            push_changes(&mut out, changes);
        }
        Record::Error {
            function,
            statement,
            message,
            changes,
        } => {
            out.push_str(&format!(
                "{{\"t\":\"error\",\"function\":{function},\"statement\":{statement},\
                 \"message\":{}",
                json_text(message)
            ));
            push_changes(&mut out, changes);
        }
        Record::String { value, text } => {
            out.push_str(&format!(
                "{{\"t\":\"string\",\"value\":\"{value:08x}\",\"text\":{}",
                json_text(text)
            ));
        }
    }
    out.push_str("}\n");
    out
}

fn push_changes(out: &mut String, changes: &Changes) {
    out.push_str(",\"globals\":[");
    for (i, change) in changes.globals.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!("[{},\"{:08x}\"]", change.slot, change.bits));
    }
    out.push_str("],\"fields\":[");
    for (i, change) in changes.fields.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "[{},{},\"{:08x}\"]",
            change.edict, change.field, change.bits
        ));
    }
    out.push(']');
}

/// TRACE-6.5: printable ASCII but the backslash as itself, every other byte as `\xNN`, then
/// quoted as a JSON string.
fn json_text(text: &Text) -> String {
    let mut escaped = String::new();
    for &b in text.as_bytes() {
        if (0x20..=0x7E).contains(&b) && b != b'\\' {
            escaped.push(char::from(b));
        } else {
            escaped.push_str(&format!("\\x{b:02x}"));
        }
    }
    Value::String(escaped).to_string()
}

/// Decodes a whole trace from its JSON form.
///
/// Implements TRACE-6 and the reader's checks of TRACE-6.6.
///
/// # Errors
///
/// [`TraceError::Json`] with the line number for any line that breaks TRACE-6, or the
/// [`TraceError`] for a header or change set that breaks TRACE-2.3 or TRACE-5.7.
pub fn from_json(text: &str) -> Result<Trace, TraceError> {
    let Some(body) = text.strip_suffix('\n') else {
        return Err(json_error(
            text.split('\n').count(),
            "the last line has no line feed",
        ));
    };
    let mut lines = body.split('\n').zip(1_usize..);
    let Some((first, _)) = lines.next() else {
        return Err(json_error(1, "missing header"));
    };
    let header = parse_header(first).map_err(|e| e.at(1))?;
    let mut records = Vec::new();
    for (line, number) in lines {
        records.push(parse_record(line).map_err(|e| e.at(number))?);
    }
    Ok(Trace { header, records })
}

fn json_error(line: usize, reason: &str) -> TraceError {
    TraceError::Json {
        line,
        reason: reason.to_owned(),
    }
}

/// An error found in one line, before its line number is known.
enum LineError {
    Json(String),
    Trace(TraceError),
}

impl LineError {
    fn at(self, line: usize) -> TraceError {
        match self {
            Self::Json(reason) => TraceError::Json { line, reason },
            Self::Trace(error) => error,
        }
    }
}

impl From<TraceError> for LineError {
    fn from(error: TraceError) -> Self {
        Self::Trace(error)
    }
}

fn bad(reason: impl Into<String>) -> LineError {
    LineError::Json(reason.into())
}

/// One line's object, with each key taken out as it is read so leftovers are unknown keys.
struct Object(Map<String, Value>);

impl Object {
    fn parse(line: &str) -> Result<Self, LineError> {
        if line.is_empty() {
            return Err(bad("blank line"));
        }
        match serde_json::from_str::<Value>(line) {
            Ok(Value::Object(map)) => Ok(Self(map)),
            Ok(_) => Err(bad("not a JSON object")),
            Err(e) => Err(bad(format!("invalid JSON: {e}"))),
        }
    }

    fn take(&mut self, key: &str) -> Result<Value, LineError> {
        self.0
            .remove(key)
            .ok_or_else(|| bad(format!("missing key `{key}`")))
    }

    fn string(&mut self, key: &str) -> Result<String, LineError> {
        match self.take(key)? {
            Value::String(s) => Ok(s),
            _ => Err(bad(format!("`{key}` is not a string"))),
        }
    }

    fn u32(&mut self, key: &str) -> Result<u32, LineError> {
        let value = self.take(key)?;
        to_u32(&value).ok_or_else(|| bad(format!("`{key}` is not an integer from 0 to 4294967295")))
    }

    fn i32(&mut self, key: &str) -> Result<i32, LineError> {
        let value = self.take(key)?;
        value
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .ok_or_else(|| bad(format!("`{key}` is not a 32-bit signed integer")))
    }

    fn bits(&mut self, key: &str) -> Result<u32, LineError> {
        let value = self.take(key)?;
        parse_bits(&value).ok_or_else(|| bad(format!("`{key}` is not 8 lowercase hex digits")))
    }

    fn text(&mut self, key: &str) -> Result<Text, LineError> {
        let s = self.string(key)?;
        let bytes = unescape_text(&s).ok_or_else(|| bad(format!("`{key}` is not escaped text")))?;
        Ok(Text::new(bytes)?)
    }

    fn changes(&mut self) -> Result<Changes, LineError> {
        let Value::Array(globals_json) = self.take("globals")? else {
            return Err(bad("`globals` is not an array"));
        };
        let mut globals = Vec::with_capacity(globals_json.len());
        for item in &globals_json {
            match item.as_array().map(Vec::as_slice) {
                Some([slot, bits]) => {
                    let slot = to_u32(slot).ok_or_else(|| bad("a global slot is out of range"))?;
                    let bits =
                        parse_bits(bits).ok_or_else(|| bad("a global's bits are malformed"))?;
                    globals.push(GlobalChange { slot, bits });
                }
                _ => return Err(bad("a global change is not [slot, bits]")),
            }
        }
        let Value::Array(fields_json) = self.take("fields")? else {
            return Err(bad("`fields` is not an array"));
        };
        let mut fields = Vec::with_capacity(fields_json.len());
        for item in &fields_json {
            match item.as_array().map(Vec::as_slice) {
                Some([edict, field, bits]) => {
                    let edict =
                        to_u32(edict).ok_or_else(|| bad("an edict number is out of range"))?;
                    let field = to_u32(field).ok_or_else(|| bad("a field slot is out of range"))?;
                    let bits =
                        parse_bits(bits).ok_or_else(|| bad("a field's bits are malformed"))?;
                    fields.push(FieldChange { edict, field, bits });
                }
                _ => return Err(bad("a field change is not [edict, field, bits]")),
            }
        }
        Ok(Changes::from_sorted(globals, fields)?)
    }

    fn finish(self) -> Result<(), LineError> {
        match self.0.keys().next() {
            None => Ok(()),
            Some(key) => Err(bad(format!("unknown key `{key}`"))),
        }
    }
}

fn to_u32(value: &Value) -> Option<u32> {
    value.as_u64().and_then(|n| u32::try_from(n).ok())
}

/// TRACE-6.4: exactly 8 lowercase hexadecimal digits.
fn parse_bits(value: &Value) -> Option<u32> {
    let s = value.as_str()?;
    let well_formed = s.len() == 8
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !well_formed {
        return None;
    }
    u32::from_str_radix(s, 16).ok()
}

fn hex_digit(c: char) -> Option<u8> {
    match c {
        '0'..='9' | 'a'..='f' => c.to_digit(16).and_then(|d| u8::try_from(d).ok()),
        _ => None,
    }
}

/// TRACE-6.5 reversed. Only the one encoding of each byte is accepted: a printable byte
/// written as `\xNN` is rejected, so decoding and encoding again gives the same text.
fn unescape_text(s: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if chars.next() != Some('x') {
                return None;
            }
            let high = hex_digit(chars.next()?)?;
            let low = hex_digit(chars.next()?)?;
            let b = high.checked_mul(16)?.checked_add(low)?;
            if (0x20..=0x7E).contains(&b) && b != b'\\' {
                return None;
            }
            bytes.push(b);
        } else {
            let b = u8::try_from(u32::from(c)).ok()?;
            if !(0x20..=0x7E).contains(&b) {
                return None;
            }
            bytes.push(b);
        }
    }
    Some(bytes)
}

fn parse_header(line: &str) -> Result<Header, LineError> {
    let mut object = Object::parse(line)?;
    if object.string("trace")? != FORMAT_NAME {
        return Err(bad("`trace` is not \"qc-trace\""));
    }
    let version = object.u32("version")?;
    if version != TRACE_VERSION {
        return Err(TraceError::UnsupportedVersion(version).into());
    }
    let crc = object.u32("progs_crc")?;
    let edict_size = object.u32("edict_size")?;
    let field_offset = object.u32("field_offset")?;
    let entityfields = object.u32("entityfields")?;
    let numstrings = object.u32("numstrings")?;
    object.finish()?;
    let crc = u16::try_from(crc).map_err(|_| TraceError::BadHeader)?;
    Ok(Header::new(
        crc,
        edict_size,
        field_offset,
        entityfields,
        numstrings,
    )?)
}

fn parse_record(line: &str) -> Result<Record, LineError> {
    let mut object = Object::parse(line)?;
    let kind = object.string("t")?;
    let record = match kind.as_str() {
        "run" => {
            let function = object.i32("function")?;
            let changes = object.changes()?;
            Record::Run { function, changes }
        }
        "step" => {
            let function = object.i32("function")?;
            let statement = object.i32("statement")?;
            let opcode = object.u32("opcode")?;
            let opcode = u16::try_from(opcode).map_err(|_| TraceError::OpcodeOutOfRange(opcode))?;
            let builtin = if object.0.contains_key("builtin") {
                Some(object.u32("builtin")?)
            } else {
                None
            };
            let changes = object.changes()?;
            Record::Step(Step {
                function,
                statement,
                opcode,
                builtin,
                changes,
            })
        }
        "exit" => Record::Exit {
            changes: object.changes()?,
        },
        "error" => {
            let function = object.i32("function")?;
            let statement = object.i32("statement")?;
            let message = object.text("message")?;
            let changes = object.changes()?;
            Record::Error {
                function,
                statement,
                message,
                changes,
            }
        }
        "string" => {
            let value = object.bits("value")?;
            let text = object.text("text")?;
            Record::String { value, text }
        }
        other => return Err(bad(format!("unknown record type `{other}`"))),
    };
    object.finish()?;
    Ok(record)
}
