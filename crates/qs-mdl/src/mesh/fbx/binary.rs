// SPDX-License-Identifier: GPL-3.0-or-later

//! The binary form of `.fbx` (MDL-11), written from the format's description, nothing
//! ported: a file is a tree of node records, each a name, properties and nested records,
//! which is what the text form writes as `Name: properties {` and `}`. [`text`] writes a
//! binary file's tree as the text form, which [`super::read`] then reads, so one reader of
//! objects and connections serves both.

use core::fmt::Write as _;
use std::collections::BTreeMap;

use super::{FBX_VERSION, FbxError};

/// What a binary file starts with: "Bytes 0 - 20: Kaydara FBX Binary  \x00 (file-magic, with
/// 2 spaces at the end, then a NULL terminator)".
pub(super) const FBX_BINARY_MAGIC: &[u8; 21] = b"Kaydara FBX Binary  \0";
/// The bytes of the header: the magic, two more, and the version, "Bytes 23 - 26: unsigned
/// int, the version number. 7300 for version 7.3 for example."
const HEADER: usize = 27;
const VERSION_AT: usize = 23;
/// The versions read, those of FBX 7.
const VERSIONS: core::ops::Range<u32> = 7000..8000;
/// The version from which a record's three counts are 64 bits each, not 32: "FBX 7500 (aka
/// FBX2016) introduces incompatible changes at binary level".
const WIDE_VERSION: u32 = 7500;
/// How deep records may be nested. Ours: a file's own nest far less.
const MAX_DEPTH: usize = 64;
/// The most bytes the arrays of a file may hold once inflated. Ours: an array's length is
/// the file's word, and a deflated one may say anything.
pub(super) const FBX_MAX_ARRAY_BYTES: usize = 1 << 26;
/// The bytes of an array's value: 8 for a double or a 64-bit integer, 4 for a float or a
/// 32-bit integer, 1 for a boolean or a byte.
const WIDE: usize = 8;
const NARROW: usize = 4;
/// An array's `Encoding` when its contents are deflated: "If Encoding is 1, the Contents is
/// a deflate/zip-compressed buffer".
const DEFLATED: u32 = 1;
/// The greatest whole number a double holds exactly, 2 to the 53rd: an object's number
/// above it is given a number of its own.
const EXACT: i64 = 1 << 53;

/// The bytes of a file being read.
struct Bytes<'a> {
    all: &'a [u8],
    at: usize,
}

impl<'a> Bytes<'a> {
    /// The next `count` bytes; `None` when the file is cut short.
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(count)?;
        let bytes = self.all.get(self.at..end)?;
        self.at = end;
        Some(bytes)
    }

    /// The next bytes as an array of `N`.
    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }

    fn u8(&mut self) -> Option<u8> {
        self.array::<1>().map(u8::from_le_bytes)
    }

    fn u32(&mut self) -> Option<u32> {
        self.array().map(u32::from_le_bytes)
    }

    /// A count of a record: 32 bits, or 64 in a file of [`WIDE_VERSION`] or later.
    fn count(&mut self, width: Width) -> Option<usize> {
        match width {
            Width::Narrow => usize::try_from(self.u32()?).ok(),
            Width::Wide => usize::try_from(u64::from_le_bytes(self.array()?)).ok(),
        }
    }
}

/// How wide a record's counts are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Width {
    Narrow,
    Wide,
}

/// What is written of a file as it is read.
struct Writer {
    out: String,
    /// The object numbers too great for a double, each with the number it is written as.
    renumbered: BTreeMap<i64, usize>,
    /// How many bytes of arrays may still be inflated.
    budget: usize,
}

impl Writer {
    /// A whole number as the text form has one. An object's number is a 64-bit integer, and
    /// the text form's reader keeps numbers as doubles: one too great to be kept exactly is
    /// written as a number no whole number is, its own, so that two are never taken for one.
    fn whole(&mut self, value: i64) {
        if value.unsigned_abs() <= EXACT.unsigned_abs() {
            let _infallible = write!(self.out, "{value}");
        } else {
            let next = self.renumbered.len();
            let number = *self.renumbered.entry(value).or_insert(next);
            let _infallible = write!(self.out, "-{number}.5");
        }
    }

    /// A number with a fraction; one that is no number is 0.
    fn real(&mut self, value: f64) {
        let value = if value.is_finite() { value } else { 0.0 };
        let _infallible = write!(self.out, "{value}");
    }

    /// A string as the text form quotes one. A binary file writes an object's name and its
    /// class as the name, a NUL, a byte of 1 and the class; the text form writes them as
    /// `Class::Name`. A quote or a line's end in a string is written as a space.
    fn string(&mut self, bytes: &[u8]) {
        let (name, class) = match bytes.iter().position(|byte| *byte == 0) {
            Some(nul) => (
                bytes.get(..nul).unwrap_or(bytes),
                bytes.get(nul.saturating_add(2)..),
            ),
            None => (bytes, None),
        };
        let clean = |bytes: &[u8]| -> String {
            String::from_utf8_lossy(bytes)
                .chars()
                .map(|c| if c == '"' || c.is_control() { ' ' } else { c })
                .collect()
        };
        self.out.push('"');
        if let Some(class) = class {
            self.out.push_str(&clean(class));
            self.out.push_str("::");
        }
        self.out.push_str(&clean(name));
        self.out.push('"');
    }

    /// A property of the kind `code`: a scalar, an array, a string or raw bytes.
    fn property(&mut self, bytes: &mut Bytes<'_>, code: u8) -> Option<()> {
        match code {
            b'Y' => self.whole(i64::from(i16::from_le_bytes(bytes.array()?))),
            b'C' | b'B' | b'Z' => self.whole(i64::from(bytes.u8()?)),
            b'I' => self.whole(i64::from(i32::from_le_bytes(bytes.array()?))),
            b'L' => self.whole(i64::from_le_bytes(bytes.array()?)),
            b'F' => self.real(f64::from(f32::from_le_bytes(bytes.array()?))),
            b'D' => self.real(f64::from_le_bytes(bytes.array()?)),
            b'S' => {
                let length = usize::try_from(bytes.u32()?).ok()?;
                self.string(bytes.take(length)?);
            }
            b'R' => {
                let length = usize::try_from(bytes.u32()?).ok()?;
                bytes.take(length)?;
                self.out.push_str("\"\"");
            }
            b'f' | b'd' | b'l' | b'i' | b'b' | b'c' => self.array(bytes, code)?,
            _ => return None,
        }
        Some(())
    }

    /// An array: its length, its encoding and its stored length, then its contents, as they
    /// are or deflated. It is written as the text form's `*N { a: ... }`.
    fn array(&mut self, bytes: &mut Bytes<'_>, code: u8) -> Option<()> {
        let length = usize::try_from(bytes.u32()?).ok()?;
        let encoding = bytes.u32()?;
        let stored = usize::try_from(bytes.u32()?).ok()?;
        let stride: usize = match code {
            b'd' | b'l' => WIDE,
            b'f' | b'i' => NARROW,
            _ => 1,
        };
        let size = length.checked_mul(stride)?;
        if size > self.budget {
            return None;
        }
        self.budget = self.budget.saturating_sub(size);
        let contents = bytes.take(stored)?;
        let inflated;
        let contents = if encoding == DEFLATED {
            inflated =
                miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(contents, size).ok()?;
            inflated.as_slice()
        } else {
            contents
        };
        if contents.len() != size {
            return None;
        }
        let _infallible = write!(self.out, "*{length} {{\na: ");
        for (place, value) in contents.chunks_exact(stride).enumerate() {
            if place > 0 {
                self.out.push(',');
            }
            match code {
                b'd' => self.real(f64::from_le_bytes(value.try_into().ok()?)),
                b'f' => self.real(f64::from(f32::from_le_bytes(value.try_into().ok()?))),
                b'l' => self.whole(i64::from_le_bytes(value.try_into().ok()?)),
                b'i' => self.whole(i64::from(i32::from_le_bytes(value.try_into().ok()?))),
                _ => self.whole(i64::from(*value.first()?)),
            }
        }
        self.out.push_str("\n}");
        Some(())
    }

    /// The records from where `bytes` stands to the record of all zeros that ends a list, or
    /// to `end`: each a line `Name: properties`, with the records nested in it between `{`
    /// and `}`. `None` for a file cut short, a record that ends before it starts or past
    /// its list's end, a property of no kind, and records nested too deep.
    fn records(
        &mut self,
        bytes: &mut Bytes<'_>,
        width: Width,
        end: usize,
        depth: usize,
    ) -> Option<()> {
        if depth > MAX_DEPTH {
            return None;
        }
        while bytes.at < end {
            let record_end = bytes.count(width)?;
            let properties = bytes.count(width)?;
            let _properties_length = bytes.count(width)?;
            let name = bytes.u8()?;
            let name = bytes.take(usize::from(name))?;
            if record_end == 0 {
                // "NestedList is the nested list, presence of which is indicated by a
                // NULL-record at the very end."
                return Some(());
            }
            if record_end > end || record_end < bytes.at {
                return None;
            }
            let name: String = String::from_utf8_lossy(name)
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            let _infallible = write!(self.out, "{name}: ");
            let mut has_array = false;
            for place in 0..properties {
                if place > 0 && !has_array {
                    self.out.push(',');
                }
                let code = bytes.u8()?;
                has_array |= code.is_ascii_lowercase();
                self.property(bytes, code)?;
            }
            if bytes.at < record_end {
                self.out.push_str(" {\n");
                self.records(bytes, width, record_end, depth.saturating_add(1))?;
                self.out.push_str("}\n");
            } else {
                self.out.push('\n');
            }
            bytes.at = record_end;
        }
        Some(())
    }
}

/// Whether `bytes` is a binary `.fbx` file, by its first bytes.
pub(super) fn is_binary(bytes: &[u8]) -> bool {
    bytes.starts_with(FBX_BINARY_MAGIC)
}

/// The binary file `bytes` as the text form: a first line naming FBX 7, then its records.
///
/// Implements MDL-11.1 to MDL-11.4.
///
/// # Errors
///
/// [`FbxError::Version`] for a file that is not of FBX 7, and [`FbxError::Binary`] for one
/// that is cut short, has a record or a property that cannot be, an array that does not
/// inflate to its length, or more in its arrays than [`FBX_MAX_ARRAY_BYTES`].
pub(super) fn text(bytes: &[u8]) -> Result<String, FbxError> {
    let version = bytes
        .get(VERSION_AT..HEADER)
        .and_then(|four| four.try_into().ok())
        .map(u32::from_le_bytes)
        .filter(|version| VERSIONS.contains(version))
        .ok_or(FbxError::Version)?;
    let width = if version < WIDE_VERSION {
        Width::Narrow
    } else {
        Width::Wide
    };
    let mut writer = Writer {
        out: format!("; {FBX_VERSION} binary file\n"),
        renumbered: BTreeMap::new(),
        budget: FBX_MAX_ARRAY_BYTES,
    };
    let mut bytes = Bytes {
        all: bytes,
        at: HEADER,
    };
    let end = bytes.all.len();
    writer
        .records(&mut bytes, width, end, 0)
        .ok_or(FbxError::Binary)?;
    Ok(writer.out)
}
