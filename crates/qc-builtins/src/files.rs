// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2016      Spike
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from Novum/vkQuake@27789d7333a46674474d871137bb71e056719628: Quake/pr_ext.c, QC_FixFileName, PF_fopen, PF_fgets, PF_fputs, PF_fclose, PF_whichpack

//! vkQuake's file builtins over the mock's game directory (BUILTIN-18.36 to BUILTIN-18.38): the
//! directory starts empty, and a file QuakeC writes is kept in memory, under `data/`, for the
//! rest of the run, so a later read finds it as vkQuake's would.

use std::collections::BTreeMap;

use qc_vm::convert::float_to_int;

/// `QC_FILE_BASE`, the handle of the first open file.
const QC_FILE_BASE: usize = 1;
/// vkQuake's `STRINGTEMP_LENGTH`, the most bytes `fgets` returns, its zero among them.
const STRINGTEMP_LENGTH: usize = 1024;

/// How `fopen` opens a file: its `mode` argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// 0: read.
    Read,
    /// 1: append, which vkQuake opens with `w+b`, emptying the file.
    Append,
    /// 2: write.
    Write,
}

impl Mode {
    /// The mode `fopen`'s `(int)` mode names, or `None` for an unsupported one.
    #[must_use]
    pub const fn from_number(number: i32) -> Option<Self> {
        match number {
            0 => Some(Self::Read),
            1 => Some(Self::Append),
            2 => Some(Self::Write),
            _ => None,
        }
    }
}

/// One open file, `struct qcfile_s`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Open {
    path: Vec<u8>,
    mode: Mode,
    /// Reading: the bytes the file had when it was opened. Writing: the bytes written so far,
    /// which the file holds once it is closed.
    data: Vec<u8>,
    /// How many bytes were read.
    offset: usize,
}

/// The game directory's files QuakeC wrote and the open file table (BUILTIN-18.36).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Files {
    written: BTreeMap<Vec<u8>, Vec<u8>>,
    open: Vec<Option<Open>>,
}

/// `QC_FixFileName`: the path `name` is read or written at, `data/` and the name, and the name
/// itself when it may also be read there; `None` for a refused name (empty, holding `:` or
/// `\`, starting with `/`, or holding `..`).
fn fix_file_name(name: &[u8]) -> Option<(Vec<u8>, Option<Vec<u8>>)> {
    if name.is_empty()
        || name.contains(&b':')
        || name.contains(&b'\\')
        || name.first() == Some(&b'/')
        || name.windows(2).any(|pair| pair == b"..")
    {
        return None;
    }
    let starts = |prefix: &[u8]| {
        name.get(..prefix.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
    };
    let extension = file_extension(name);
    let config = (!name.contains(&b'/') || !starts(b"configs/"))
        && extension.eq_ignore_ascii_case(b"cfg")
        && !starts(b"particles/")
        && !starts(b"huds/")
        && !starts(b"models/");
    let mut path = b"data/".to_vec();
    path.extend_from_slice(name);
    Some((path, (!config).then(|| name.to_vec())))
}

/// `COM_FileGetExtension`: the bytes after the last `.` that has a byte after it, searching
/// back from the last byte; empty for a name under two bytes, with no such `.`, or whose
/// extension holds a `/` or `\`.
fn file_extension(name: &[u8]) -> &[u8] {
    if name.len() < 2 {
        return &[];
    }
    let mut src = name.len().saturating_sub(1);
    while src != 0 && name.get(src.saturating_sub(1)) != Some(&b'.') {
        src = src.saturating_sub(1);
    }
    let extension = name.get(src..).unwrap_or_default();
    if src == 0 || extension.contains(&b'/') || extension.contains(&b'\\') {
        return &[];
    }
    extension
}

impl Files {
    /// `PF_fopen`: the handle, from 1, of `name` opened in `mode`, or -1 when it is refused, a
    /// file to read is not there, or the mode is unsupported. Writing and appending empty the
    /// file at `data/<name>`.
    pub fn open(&mut self, name: &[u8], mode: i32) -> f32 {
        let Some((path, fallback)) = fix_file_name(name) else {
            return -1.0;
        };
        let Some(mode) = Mode::from_number(mode) else {
            return -1.0;
        };
        let open = match mode {
            Mode::Read => {
                let found = self
                    .written
                    .get(&path)
                    .map(|data| (path.clone(), data))
                    .or_else(|| {
                        fallback.as_ref().and_then(|name| {
                            self.written.get(name).map(|data| (name.clone(), data))
                        })
                    });
                let Some((path, data)) = found else {
                    return -1.0;
                };
                Open {
                    path,
                    mode,
                    data: data.clone(),
                    offset: 0,
                }
            }
            Mode::Append | Mode::Write => {
                // `w+b` or `wb`: the file is made, or emptied, at once.
                self.written.insert(path.clone(), Vec::new());
                Open {
                    path,
                    mode,
                    data: Vec::new(),
                    offset: 0,
                }
            }
        };
        let index = self
            .open
            .iter()
            .position(Option::is_none)
            .unwrap_or(self.open.len());
        if index == self.open.len() {
            self.open.push(None);
        }
        if let Some(slot) = self.open.get_mut(index) {
            *slot = Some(open);
        }
        let handle = index.saturating_add(QC_FILE_BASE);
        f32::from(u16::try_from(handle).unwrap_or(u16::MAX))
    }

    /// The open file of handle `handle`, C's `size_t` of the float less `QC_FILE_BASE`.
    fn slot(&mut self, handle: f32) -> Option<&mut Open> {
        let index = handle_index(handle, self.open.len())?;
        self.open.get_mut(index)?.as_mut()
    }

    /// `PF_fgets`: the next line of a file open to read, without its line feed or the carriage
    /// return before one, cut to 1023 bytes; `None` at the end, or for a handle that is not a
    /// file open to read.
    pub fn gets(&mut self, handle: f32) -> Option<Vec<u8>> {
        let open = self.slot(handle).filter(|open| open.mode == Mode::Read)?;
        let rest = open.data.get(open.offset..).unwrap_or_default();
        if rest.is_empty() {
            return None;
        }
        let line_end = rest.iter().position(|&b| b == b'\n');
        let consumed = line_end.map_or(rest.len(), |at| at.saturating_add(1));
        let mut line = rest
            .get(..line_end.unwrap_or(rest.len()))
            .unwrap_or_default()
            .to_vec();
        // `if (s == end) s--` keeps writing the last byte, which the terminator then overwrites,
        // so a longer line keeps its first 1023 bytes.
        line.truncate(STRINGTEMP_LENGTH.saturating_sub(1));
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        open.offset = open.offset.saturating_add(consumed);
        Some(line)
    }

    /// `PF_fputs`: `text` added to a file open to write or append, held in its buffer until
    /// it is closed; nothing for any other handle.
    pub fn puts(&mut self, handle: f32, text: &[u8]) {
        if let Some(open) = self.slot(handle).filter(|open| open.mode != Mode::Read) {
            open.data.extend_from_slice(text);
        }
    }

    /// `PF_fclose`: the handle's file closed, a written one's text then in the file; nothing
    /// for a handle that is not open.
    pub fn close(&mut self, handle: f32) {
        let Some(index) = handle_index(handle, self.open.len()) else {
            return;
        };
        let Some(open) = self.open.get_mut(index).and_then(Option::take) else {
            return;
        };
        if open.mode != Mode::Read {
            self.written.insert(open.path, open.data);
        }
    }

    /// `PF_whichpack`: the game directory, `id1`, when a file QuakeC wrote is at `name`, else
    /// `None`.
    #[must_use]
    pub fn which_pack(&self, name: &[u8]) -> Option<&'static [u8]> {
        self.written.contains_key(name).then_some(b"id1".as_slice())
    }
}

/// `(size_t)(handle - QC_FILE_BASE)` of the float, when it is below `len`: the float less 1
/// truncated toward zero, so from -1 to 0 exclusive it is 0; a NaN or a value at -1 or below
/// is no index (C's conversion of it is undefined, and none of its results is below `len` on
/// x86-64).
fn handle_index(handle: f32, len: usize) -> Option<usize> {
    let index = handle - 1.0;
    // A NaN or a value at -1 or below has no slot.
    if index.is_nan() || index <= -1.0 {
        return None;
    }
    // `float_to_int` truncates toward zero; a value past an int's range is no slot either.
    usize::try_from(float_to_int(index)).ok().filter(|&n| n < len)
}

#[cfg(test)]
mod tests;
