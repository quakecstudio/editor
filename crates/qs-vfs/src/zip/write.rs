// SPDX-License-Identifier: GPL-3.0-or-later

//! Writing a zip archive, for a mod exported for players (P4-28, QA-15): each member's local
//! header and data, then the central directory and its end record, as PKWARE's APPNOTE 6.3.10
//! lays them out (<https://pkware.cachefly.net/webdocs/APPNOTE/APPNOTE-6.3.10.TXT>, sections
//! 4.3.7, 4.3.12 and 4.3.16; its terms forbid a copy in `specs/`, so it is cited here).
//!
//! The archive is the same bytes for the same members: every member has the same time, the
//! earliest an MS-DOS date holds (4.4.6), no extra field and no comment, and is deflated at one
//! fixed level, or stored when deflating does not make it smaller. Zip64 is not written, so a
//! member or the archive past 4 GiB, or more than 65535 members, is an error.

use std::fmt;

use miniz_oxide::deflate::compress_to_vec;

use super::{CENTRAL, DEFLATED, EOCD, LOCAL, STORED, crc32};

/// The deflate level every member is written at.
const LEVEL: u8 = 9;

/// "Version needed to extract" (4.4.3.2): 1.0 for a stored member, 2.0 for a deflated one.
const STORED_VERSION: u16 = 10;
/// As [`STORED_VERSION`], for a deflated member.
const DEFLATED_VERSION: u16 = 20;
/// "Version made by" (4.4.2): MS-DOS attributes (high byte 0), APPNOTE 2.0.
const MADE_BY: u16 = 20;
/// General purpose bit 11 (4.4.4): the member's name is UTF-8.
const UTF8_NAME: u16 = 1 << 11;
/// The MS-DOS date of 1980-01-01 (4.4.6): day 1, month 1, year 0 from 1980.
const DOS_DATE: u16 = (1 << 5) | 1;

/// Why a zip could not be written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ZipWriteError {
    /// A member's name is empty, absolute, or holds a `\`, a `.` or `..` part, or a NUL.
    BadName(String),
    /// Two members have the same name.
    Duplicate(String),
    /// The archive or a member is too large for a zip without zip64, or has more than 65535
    /// members.
    TooLarge,
}

impl fmt::Display for ZipWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadName(name) => write!(f, "{name:?} is not a name a zip member can have"),
            Self::Duplicate(name) => write!(f, "two members are named {name:?}"),
            Self::TooLarge => f.write_str("the archive needs zip64, which is not written"),
        }
    }
}

impl core::error::Error for ZipWriteError {}

/// One member to write: its name, `/` between its parts, and its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewMember {
    /// The name, such as `mymod/progs.dat`.
    pub name: String,
    /// The bytes.
    pub data: Vec<u8>,
}

/// A member as written: what its central directory header repeats.
struct Written {
    name: String,
    method: u16,
    version: u16,
    flags: u16,
    crc: u32,
    compressed: u32,
    size: u32,
    offset: u32,
}

/// The zip of `members`, in the order given.
///
/// # Errors
///
/// [`ZipWriteError`] for a bad or repeated name, or an archive that needs zip64.
pub fn write_zip(members: &[NewMember]) -> Result<Vec<u8>, ZipWriteError> {
    let too_large = |_| ZipWriteError::TooLarge;
    let count = u16::try_from(members.len()).map_err(too_large)?;
    let mut out = Vec::new();
    let mut written: Vec<Written> = Vec::with_capacity(members.len());
    for member in members {
        check_name(&member.name)?;
        if written.iter().any(|w| w.name == member.name) {
            return Err(ZipWriteError::Duplicate(member.name.clone()));
        }
        let deflated = compress_to_vec(&member.data, LEVEL);
        let (method, version, body) = if deflated.len() < member.data.len() {
            (DEFLATED, DEFLATED_VERSION, deflated)
        } else {
            (STORED, STORED_VERSION, member.data.clone())
        };
        let flags = if member.name.is_ascii() { 0 } else { UTF8_NAME };
        let entry = Written {
            name: member.name.clone(),
            method,
            version,
            flags,
            crc: crc32(&member.data),
            compressed: u32::try_from(body.len()).map_err(too_large)?,
            size: u32::try_from(member.data.len()).map_err(too_large)?,
            offset: u32::try_from(out.len()).map_err(too_large)?,
        };
        // 4.3.7: the local file header, then the name and the data.
        put32(&mut out, LOCAL);
        put16(&mut out, entry.version);
        put_common(&mut out, &entry)?;
        out.extend_from_slice(entry.name.as_bytes());
        out.extend_from_slice(&body);
        written.push(entry);
    }
    let central_start = u32::try_from(out.len()).map_err(too_large)?;
    for entry in &written {
        // 4.3.12: the central directory header of each member.
        put32(&mut out, CENTRAL);
        put16(&mut out, MADE_BY);
        put16(&mut out, entry.version);
        put_common(&mut out, entry)?;
        put16(&mut out, 0); // file comment length
        put16(&mut out, 0); // disk number start
        put16(&mut out, 0); // internal file attributes
        put32(&mut out, 0); // external file attributes
        put32(&mut out, entry.offset);
        out.extend_from_slice(entry.name.as_bytes());
    }
    let central_size = u32::try_from(out.len())
        .map_err(too_large)?
        .checked_sub(central_start)
        .ok_or(ZipWriteError::TooLarge)?;
    // 4.3.16: the end of central directory record, on disk 0, with no comment.
    put32(&mut out, EOCD);
    put16(&mut out, 0);
    put16(&mut out, 0);
    put16(&mut out, count);
    put16(&mut out, count);
    put32(&mut out, central_size);
    put32(&mut out, central_start);
    put16(&mut out, 0);
    u32::try_from(out.len()).map_err(too_large)?;
    Ok(out)
}

/// The fields a local header and a central directory header share, from the flags to the
/// extra field's length.
fn put_common(out: &mut Vec<u8>, entry: &Written) -> Result<(), ZipWriteError> {
    put16(out, entry.flags);
    put16(out, entry.method);
    put16(out, 0); // last mod file time: 00:00:00
    put16(out, DOS_DATE);
    put32(out, entry.crc);
    put32(out, entry.compressed);
    put32(out, entry.size);
    put16(
        out,
        u16::try_from(entry.name.len()).map_err(|_| ZipWriteError::TooLarge)?,
    );
    put16(out, 0); // extra field length
    Ok(())
}

/// Refuses a name that is not a relative path of `/`-separated parts (4.4.17.1: "The path
/// stored MUST NOT contain a drive or device letter, or a leading slash. All slashes MUST be
/// forward slashes '/'").
fn check_name(name: &str) -> Result<(), ZipWriteError> {
    let bad = name.is_empty()
        || name.starts_with('/')
        || name.contains(['\\', '\0', ':'])
        || name.split('/').any(|part| matches!(part, "" | "." | ".."));
    if bad {
        Err(ZipWriteError::BadName(name.to_owned()))
    } else {
        Ok(())
    }
}

/// `value`, little-endian.
fn put16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// `value`, little-endian.
fn put32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}
