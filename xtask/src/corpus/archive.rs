// SPDX-License-Identifier: GPL-3.0-or-later

//! The archive formats the corpus arrives in: a repository at a commit as a gzipped tar (RFC
//! 1952 around POSIX ustar, with pax and GNU long names, which `git archive` writes), and a
//! release as a zip (PKWARE APPNOTE, stored or deflated members). Inflating is `miniz_oxide`'s;
//! the containers are read here. Every malformed input is an error, never a panic.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

use miniz_oxide::inflate::stream::{InflateState, inflate};
use miniz_oxide::inflate::{TINFLStatus, decompress_to_vec_with_limit};
use miniz_oxide::{DataFormat, MZError, MZFlush, MZStatus};

/// RFC 1952's magic, and its only compression method (deflate).
const GZIP_MAGIC: [u8; 3] = [0x1f, 0x8b, 8];
const FHCRC: u8 = 1 << 1;
const FEXTRA: u8 = 1 << 2;
const FNAME: u8 = 1 << 3;
const FCOMMENT: u8 = 1 << 4;
const BLOCK: usize = 512;
const CHUNK: usize = 1 << 16;

/// A reader of the data inside a gzip stream: one member, its trailer's length checked.
struct Gunzip<R: Read> {
    inner: R,
    state: Box<InflateState>,
    input: Vec<u8>,
    start: usize,
    end: usize,
    eof: bool,
    done: bool,
    written: u32,
}

impl<R: Read> Gunzip<R> {
    fn new(mut inner: R) -> io::Result<Self> {
        let mut head = [0_u8; 10];
        inner.read_exact(&mut head)?;
        if head.get(..3) != Some(GZIP_MAGIC.as_slice()) {
            return Err(invalid("not a gzip stream"));
        }
        let flags = head.get(3).copied().unwrap_or_default();
        if flags & FEXTRA != 0 {
            let mut len = [0_u8; 2];
            inner.read_exact(&mut len)?;
            io::copy(
                &mut (&mut inner).take(u64::from(u16::from_le_bytes(len))),
                &mut io::sink(),
            )?;
        }
        for flag in [FNAME, FCOMMENT] {
            if flags & flag != 0 {
                skip_to_nul(&mut inner)?;
            }
        }
        if flags & FHCRC != 0 {
            inner.read_exact(&mut [0_u8; 2])?;
        }
        Ok(Self {
            inner,
            state: InflateState::new_boxed(DataFormat::Raw),
            input: vec![0; CHUNK],
            start: 0,
            end: 0,
            eof: false,
            done: false,
            written: 0,
        })
    }

    /// Reads the trailer after the deflate stream: CRC-32, then the length modulo 2^32.
    fn finish(&mut self) -> io::Result<()> {
        let mut trailer = Vec::new();
        trailer.extend_from_slice(self.input.get(self.start..self.end).unwrap_or_default());
        (&mut self.inner)
            .take(8_u64.saturating_sub(u64::try_from(trailer.len()).unwrap_or(8)))
            .read_to_end(&mut trailer)?;
        let isize = trailer
            .get(4..8)
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .map(u32::from_le_bytes)
            .ok_or_else(|| invalid("truncated gzip trailer"))?;
        if isize != self.written {
            return Err(invalid("gzip length does not match its trailer"));
        }
        self.done = true;
        Ok(())
    }
}

impl<R: Read> Read for Gunzip<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        while !self.done && !out.is_empty() {
            if self.start == self.end && !self.eof {
                self.end = self.inner.read(&mut self.input)?;
                self.start = 0;
                self.eof = self.end == 0;
            }
            let input = self.input.get(self.start..self.end).unwrap_or_default();
            let result = inflate(&mut self.state, input, out, MZFlush::None);
            self.start = self.start.saturating_add(result.bytes_consumed);
            let written = u32::try_from(result.bytes_written & 0xffff_ffff).unwrap_or_default();
            self.written = self.written.wrapping_add(written);
            match result.status {
                Ok(MZStatus::StreamEnd) => {
                    self.finish()?;
                }
                Ok(_) | Err(MZError::Buf) => {
                    if result.bytes_consumed == 0 && result.bytes_written == 0 && self.eof {
                        return Err(invalid("truncated gzip stream"));
                    }
                }
                Err(_) => return Err(invalid("corrupt deflate data")),
            }
            if result.bytes_written > 0 {
                return Ok(result.bytes_written);
            }
        }
        Ok(0)
    }
}

fn skip_to_nul(reader: &mut impl Read) -> io::Result<()> {
    let mut byte = [0_u8; 1];
    loop {
        reader.read_exact(&mut byte)?;
        if byte == [0] {
            return Ok(());
        }
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
}

/// Reads a gzipped tar and returns every regular file `keep` accepts, by its full path in the
/// archive, in archive order.
pub(crate) fn read_tar_gz(
    reader: impl Read,
    keep: &dyn Fn(&str) -> bool,
) -> io::Result<Vec<(String, Vec<u8>)>> {
    let mut gunzip = Gunzip::new(reader)?;
    let files = read_tar(&mut gunzip, keep)?;
    // Read to the end of the stream, so a truncated or corrupt archive fails even after the
    // tar's end-of-archive block.
    io::copy(&mut gunzip, &mut io::sink())?;
    Ok(files)
}

/// Reads a tar: ustar headers with their `prefix`, pax `path` records and GNU long names.
fn read_tar(
    mut reader: impl Read,
    keep: &dyn Fn(&str) -> bool,
) -> io::Result<Vec<(String, Vec<u8>)>> {
    let mut files = Vec::new();
    let mut long_name: Option<String> = None;
    let mut header = [0_u8; BLOCK];
    loop {
        if read_block(&mut reader, &mut header)? == 0 {
            return Err(invalid("tar ends with no end-of-archive block"));
        }
        if header.iter().all(|&b| b == 0) {
            return Ok(files);
        }
        check_header(&header)?;
        let size = octal(field(&header, 124, 12))?;
        let padded = size
            .checked_add(511)
            .map(|n| n & !511)
            .ok_or_else(|| invalid("tar entry too large"))?;
        let kind = header.get(156).copied().unwrap_or_default();
        let name = long_name.take().unwrap_or_else(|| ustar_name(&header));
        let wanted = match kind {
            b'x' | b'L' => true,
            b'0' | 0 | b'7' => keep(&name),
            _ => false,
        };
        let data = if wanted {
            let mut data = Vec::new();
            (&mut reader).take(size).read_to_end(&mut data)?;
            if u64::try_from(data.len()).ok() != Some(size) {
                return Err(invalid("truncated tar entry"));
            }
            io::copy(
                &mut (&mut reader).take(padded.saturating_sub(size)),
                &mut io::sink(),
            )?;
            data
        } else {
            let skipped = io::copy(&mut (&mut reader).take(padded), &mut io::sink())?;
            if skipped != padded {
                return Err(invalid("truncated tar entry"));
            }
            Vec::new()
        };
        match kind {
            b'x' => long_name = pax_path(&data),
            b'L' => {
                let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
                long_name =
                    Some(String::from_utf8_lossy(data.get(..end).unwrap_or_default()).into_owned());
            }
            _ if wanted => files.push((name, data)),
            _ => {}
        }
    }
}

/// Fills `block`, or returns 0 at a clean end of input.
fn read_block(reader: &mut impl Read, block: &mut [u8; BLOCK]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < BLOCK {
        let n = reader.read(block.get_mut(filled..).unwrap_or_default())?;
        if n == 0 {
            if filled == 0 {
                return Ok(0);
            }
            return Err(invalid("truncated tar header"));
        }
        filled = filled.saturating_add(n);
    }
    Ok(filled)
}

fn field(header: &[u8; BLOCK], at: usize, len: usize) -> &[u8] {
    header.get(at..at.saturating_add(len)).unwrap_or_default()
}

/// The header checksum: the sum of its bytes with the checksum field read as spaces.
fn check_header(header: &[u8; BLOCK]) -> io::Result<()> {
    let want = octal(field(header, 148, 8))?;
    let mut sum = 0_u64;
    for (i, &b) in header.iter().enumerate() {
        let b = if (148..156).contains(&i) { b' ' } else { b };
        sum = sum.saturating_add(u64::from(b));
    }
    if sum == want {
        Ok(())
    } else {
        Err(invalid("tar header checksum mismatch"))
    }
}

/// A NUL- or space-terminated octal number.
fn octal(bytes: &[u8]) -> io::Result<u64> {
    let mut value = 0_u64;
    for &b in bytes.iter().skip_while(|&&b| b == b' ') {
        match b {
            b'0'..=b'7' => {
                value = value
                    .checked_mul(8)
                    .and_then(|v| v.checked_add(u64::from(b.wrapping_sub(b'0'))))
                    .ok_or_else(|| invalid("tar number overflows"))?;
            }
            0 | b' ' => break,
            _ => return Err(invalid("tar number is not octal")),
        }
    }
    Ok(value)
}

fn c_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(bytes.get(..end).unwrap_or_default()).into_owned()
}

/// The name in a header, with the ustar `prefix` in front when there is one.
fn ustar_name(header: &[u8; BLOCK]) -> String {
    let name = c_string(field(header, 0, 100));
    if field(header, 257, 5) != b"ustar" {
        return name;
    }
    let prefix = c_string(field(header, 345, 155));
    if prefix.is_empty() {
        name
    } else {
        format!("{prefix}/{name}")
    }
}

/// The `path` record of a pax extended header: records of `<len> <key>=<value>\n`.
fn pax_path(data: &[u8]) -> Option<String> {
    let mut rest = data;
    let mut path = None;
    while !rest.is_empty() {
        let space = rest.iter().position(|&b| b == b' ')?;
        let len: usize = std::str::from_utf8(rest.get(..space)?).ok()?.parse().ok()?;
        let record = rest.get(space.checked_add(1)?..len)?;
        let record = record.strip_suffix(b"\n")?;
        let eq = record.iter().position(|&b| b == b'=')?;
        if record.get(..eq)? == b"path" {
            path = Some(String::from_utf8_lossy(record.get(eq.checked_add(1)?..)?).into_owned());
        }
        rest = rest.get(len..)?;
    }
    path
}

/// One member of a zip's central directory.
pub(crate) struct ZipMember {
    pub(crate) name: String,
    method: u16,
    flags: u16,
    compressed: u64,
    size: u64,
    offset: u64,
}

/// A zip archive on disk: its central directory, read once, and the file to read members from.
pub(crate) struct Zip {
    file: File,
    members: Vec<ZipMember>,
}

const EOCD: u32 = 0x0605_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const LOCAL: u32 = 0x0403_4b50;
/// The end-of-central-directory record, without its comment.
const EOCD_SIZE: usize = 22;
const MAX_COMMENT: usize = 0xffff;

impl Zip {
    /// Reads the central directory of the zip in `file`.
    pub(crate) fn open(mut file: File) -> io::Result<Self> {
        let len = file.seek(SeekFrom::End(0))?;
        let tail_len = len.min(u64::try_from(EOCD_SIZE.saturating_add(MAX_COMMENT)).unwrap_or(len));
        file.seek(SeekFrom::Start(len.saturating_sub(tail_len)))?;
        let mut tail = Vec::new();
        (&mut file).take(tail_len).read_to_end(&mut tail)?;
        let at = (0..=tail.len().saturating_sub(EOCD_SIZE))
            .rev()
            .find(|&i| u32_at(&tail, i) == Some(EOCD))
            .ok_or_else(|| invalid("not a zip file: no end of central directory"))?;
        let count = u16_at(&tail, at.saturating_add(10)).unwrap_or_default();
        let cd_size = u32_at(&tail, at.saturating_add(12)).unwrap_or_default();
        let cd_offset = u32_at(&tail, at.saturating_add(16)).unwrap_or_default();
        if count == 0xffff || cd_size == u32::MAX || cd_offset == u32::MAX {
            return Err(invalid("zip64 archives are not supported"));
        }
        if u64::from(cd_offset).saturating_add(u64::from(cd_size)) > len {
            return Err(invalid("zip central directory lies outside the file"));
        }
        file.seek(SeekFrom::Start(u64::from(cd_offset)))?;
        let mut cd = Vec::new();
        (&mut file).take(u64::from(cd_size)).read_to_end(&mut cd)?;
        let mut members = Vec::new();
        let mut at = 0_usize;
        for _ in 0..count {
            let bad = || invalid("truncated zip central directory");
            if u32_at(&cd, at) != Some(CENTRAL) {
                return Err(bad());
            }
            let flags = u16_at(&cd, at.saturating_add(8)).ok_or_else(bad)?;
            let method = u16_at(&cd, at.saturating_add(10)).ok_or_else(bad)?;
            let compressed = u32_at(&cd, at.saturating_add(20)).ok_or_else(bad)?;
            let size = u32_at(&cd, at.saturating_add(24)).ok_or_else(bad)?;
            let name_len = usize::from(u16_at(&cd, at.saturating_add(28)).ok_or_else(bad)?);
            let extra_len = usize::from(u16_at(&cd, at.saturating_add(30)).ok_or_else(bad)?);
            let comment_len = usize::from(u16_at(&cd, at.saturating_add(32)).ok_or_else(bad)?);
            let offset = u32_at(&cd, at.saturating_add(42)).ok_or_else(bad)?;
            let name_at = at.saturating_add(46);
            let name = cd
                .get(name_at..name_at.saturating_add(name_len))
                .ok_or_else(bad)?;
            if [compressed, size, offset].contains(&u32::MAX) {
                return Err(invalid("zip64 members are not supported"));
            }
            members.push(ZipMember {
                name: String::from_utf8_lossy(name).into_owned(),
                method,
                flags,
                compressed: u64::from(compressed),
                size: u64::from(size),
                offset: u64::from(offset),
            });
            at = name_at
                .saturating_add(name_len)
                .saturating_add(extra_len)
                .saturating_add(comment_len);
        }
        Ok(Self { file, members })
    }

    /// The bytes of the member called `name`, or `None` when the archive has no such member.
    pub(crate) fn read(&mut self, name: &str) -> io::Result<Option<Vec<u8>>> {
        let Some(member) = self.members.iter().find(|m| m.name == name) else {
            return Ok(None);
        };
        if member.flags & 1 != 0 {
            return Err(invalid("encrypted zip members are not supported"));
        }
        self.file.seek(SeekFrom::Start(member.offset))?;
        let mut local = [0_u8; 30];
        self.file.read_exact(&mut local)?;
        if u32_at(&local, 0) != Some(LOCAL) {
            return Err(invalid("bad zip local header"));
        }
        let skip = u16_at(&local, 26)
            .zip(u16_at(&local, 28))
            .map(|(n, e)| u64::from(n).saturating_add(u64::from(e)))
            .unwrap_or_default();
        self.file
            .seek(SeekFrom::Current(i64::try_from(skip).unwrap_or_default()))?;
        let mut data = Vec::new();
        (&mut self.file)
            .take(member.compressed)
            .read_to_end(&mut data)?;
        if u64::try_from(data.len()).ok() != Some(member.compressed) {
            return Err(invalid("truncated zip member"));
        }
        let size = usize::try_from(member.size).map_err(|_| invalid("zip member too large"))?;
        let bytes = match member.method {
            0 => data,
            8 => decompress_to_vec_with_limit(&data, size).map_err(|e| {
                invalid(match e.status {
                    TINFLStatus::HasMoreOutput => "zip member is larger than its stated size",
                    _ => "corrupt deflate data in zip member",
                })
            })?,
            _ => return Err(invalid("zip member uses an unsupported compression method")),
        };
        if bytes.len() != size {
            return Err(invalid("zip member is not its stated size"));
        }
        Ok(Some(bytes))
    }
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    let b: [u8; 2] = bytes.get(at..at.checked_add(2)?)?.try_into().ok()?;
    Some(u16::from_le_bytes(b))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let b: [u8; 4] = bytes.get(at..at.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(b))
}

#[cfg(test)]
pub(crate) mod tests;
