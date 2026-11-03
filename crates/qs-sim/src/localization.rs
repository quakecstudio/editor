// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-2001 Id Software, Inc.
// Copyright (C) 2002-2009 John Fitzgibbons and others
// Copyright (C) 2010-2014 QuakeSpasm developers
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/common.c, COM_HashString, LOC_LoadFile, LOC_GetRawString, LOC_GetString, LOC_ParseArg, LOC_HasPlaceholders, LOC_Format
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/pr_cmds.c, PF_GetStringArg, PF_VarString

//! The rerelease's localization: the keyed strings of `localization/loc_english.txt`, looked up
//! by the text the print builtins and `WriteString` send (RRE-1).
//!
//! Implements RRE-1.2 to RRE-1.8.

/// The localization file's path in the game's files (RRE-1, `LOC_Init`).
pub const RRE_LOC_FILE: &str = "localization/loc_english.txt";

/// The bytes `PF_VarString`'s and `LOC_Format`'s buffer holds, its terminating zero among them
/// (RRE-1.5).
pub const RRE_VARSTRING_LEN: usize = 1024;

/// FNV-1a's offset basis (RRE-1.2).
const RRE_FNV_OFFSET: u32 = 0x811c_9dc5;

/// FNV-1a's prime (RRE-1.2).
const RRE_FNV_PRIME: u32 = 0x0100_0193;

/// One entry: the key and the value, each a C string at an offset of the loaded text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Entry {
    key: usize,
    value: usize,
}

/// A loaded localization file, `localization_t` (RRE-1.2): the text as `LOC_LoadFile` left it,
/// the entries in file order and the open-addressed table of their indices plus one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Localization {
    text: Vec<u8>,
    entries: Vec<Entry>,
    indices: Vec<u32>,
}

/// C's `q_isblank`: a space or a tab.
const fn is_blank(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t')
}

/// C's `q_isspace`: a space, `\t`, `\n`, `\v`, `\f` or `\r`.
const fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// The C string at `at` of `text`: the bytes up to its first zero or its end.
fn c_string(text: &[u8], at: usize) -> &[u8] {
    let rest = text.get(at..).unwrap_or_default();
    rest.split(|&b| b == 0).next().unwrap_or_default()
}

/// `COM_HashString`: FNV-1a over the key's `char`s, each signed and so sign-extended from 0x80
/// up (RRE-1.8).
fn hash(key: &[u8]) -> u32 {
    let mut hash = RRE_FNV_OFFSET;
    for &byte in key {
        let char = i32::from(i8::from_ne_bytes([byte]));
        hash ^= u32::from_ne_bytes(char.to_ne_bytes());
        hash = hash.wrapping_mul(RRE_FNV_PRIME);
    }
    hash
}

impl Localization {
    /// `LOC_LoadFile` of the file's bytes (RRE-1.7): each line parsed in place in a copy of
    /// the text, as QuakeSpasm parses it, and every entry put in the table (RRE-1.2). A file
    /// with no entry loads nothing, as no file does.
    #[must_use]
    pub fn parse(file: &[u8]) -> Self {
        // `COM_LoadMallocFile` ends the text with a zero; the text ends at its first one.
        let mut text = c_string(file, 0).to_vec();
        text.push(0);
        let at = |text: &[u8], i: usize| text.get(i).copied().unwrap_or(0);
        let mut cursor = 0;
        if text.starts_with(&[0xEF, 0xBB, 0xBF]) {
            cursor = 3;
        }
        let mut entries = Vec::new();
        while at(&text, cursor) != 0 {
            while is_blank(at(&text, cursor)) {
                cursor = cursor.saturating_add(1);
            }
            let line = cursor;
            let mut equals = None;
            while at(&text, cursor) != 0 && at(&text, cursor) != b'\n' {
                if at(&text, cursor) == b'=' && equals.is_none() {
                    equals = Some(cursor);
                }
                cursor = cursor.saturating_add(1);
            }
            if at(&text, line) == b'/' {
                // A comment; a lone `/` is reported ("malformed comment") and skipped alike.
            } else if let Some(equals) = equals {
                let mut key_end = equals;
                while key_end != line && is_space(at(&text, key_end.saturating_sub(1))) {
                    key_end = key_end.saturating_sub(1);
                }
                set(&mut text, key_end, 0);
                let mut value = equals.saturating_add(1);
                while value != cursor && is_space(at(&text, value)) {
                    value = value.saturating_add(1);
                }
                let leading_quote = at(&text, value) == b'"';
                if leading_quote {
                    value = value.saturating_add(1);
                }
                let mut trailing_quote = false;
                let mut src = value;
                let mut dst = value;
                while src != cursor {
                    if at(&text, src) == b'\\' && src.saturating_add(1) != cursor {
                        let c = at(&text, src.saturating_add(1));
                        src = src.saturating_add(2);
                        let unescaped = match c {
                            b'n' => b'\n',
                            b't' => b'\t',
                            b'v' => 0x0b,
                            b'b' => 0x08,
                            b'f' => 0x0c,
                            // `\"`, `\'` and, with "unrecognized escape sequence", any other.
                            other => other,
                        };
                        set(&mut text, dst, unescaped);
                        dst = dst.saturating_add(1);
                        continue;
                    }
                    if at(&text, src) == b'"' {
                        trailing_quote = true;
                        set(&mut text, dst, 0);
                        break;
                    }
                    let byte = at(&text, src);
                    set(&mut text, dst, byte);
                    dst = dst.saturating_add(1);
                    src = src.saturating_add(1);
                }
                // RRE-1.7: the trim zeroes the byte at `dst` before it steps back, so the last
                // blank it passes stays; nothing ends a shortened value but the line's end.
                if !trailing_quote {
                    while dst != value && is_blank(at(&text, dst.saturating_sub(1))) {
                        set(&mut text, dst, 0);
                        dst = dst.saturating_sub(1);
                    }
                }
                entries.push(Entry { key: line, value });
            }
            if at(&text, cursor) != 0 {
                set(&mut text, cursor, 0);
                cursor = cursor.saturating_add(1);
            }
        }
        let size = entries.len().saturating_mul(2);
        if size == 0 {
            return Self::default();
        }
        let mut indices = vec![0_u32; size];
        for (i, entry) in entries.iter().enumerate() {
            let key = c_string(&text, entry.key);
            let start = slot(hash(key), size);
            let mut pos = start;
            // The table is twice the entries, so a free slot is always found.
            while indices.get(pos).is_some_and(|&index| index != 0) {
                pos = pos.saturating_add(1);
                if pos == size {
                    pos = 0;
                }
            }
            if let Some(index) = indices.get_mut(pos) {
                *index = u32::try_from(i.saturating_add(1)).unwrap_or(u32::MAX);
            }
        }
        Self {
            text,
            entries,
            indices,
        }
    }

    /// Whether a file with an entry is loaded (`localization.numindices`).
    #[must_use]
    pub fn is_loaded(&self) -> bool {
        !self.indices.is_empty()
    }

    /// The number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there is no entry.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `LOC_GetRawString (key)` (RRE-1.3): the value of a `$key` the table holds, else `None`.
    #[must_use]
    pub fn raw(&self, key: &[u8]) -> Option<&[u8]> {
        let key = c_string(key, 0);
        let key = key.strip_prefix(b"$")?;
        if !self.is_loaded() {
            return None;
        }
        let size = self.indices.len();
        let start = slot(hash(key), size);
        let mut pos = start;
        loop {
            let index = self.indices.get(pos).copied().unwrap_or(0);
            if index == 0 {
                return None;
            }
            let entry = usize::try_from(index)
                .ok()
                .and_then(|index| index.checked_sub(1))
                .and_then(|index| self.entries.get(index))?;
            if c_string(&self.text, entry.key) == key {
                return Some(c_string(&self.text, entry.value));
            }
            pos = pos.saturating_add(1);
            if pos == size {
                pos = 0;
            }
            if pos == start {
                return None;
            }
        }
    }

    /// `LOC_GetString (key)` (RRE-1.3): the value, or the key itself.
    #[must_use]
    pub fn get<'a>(&'a self, key: &'a [u8]) -> &'a [u8] {
        self.raw(key).unwrap_or_else(|| c_string(key, 0))
    }

    /// `LOC_HasPlaceholders` (RRE-1.4): a `{}` or `{N}` in `text`, and a file loaded.
    #[must_use]
    pub fn has_placeholders(&self, text: &[u8]) -> bool {
        if !self.is_loaded() {
            return false;
        }
        let text = c_string(text, 0);
        // `while (*str) { if (LOC_ParseArg(&str) >= 0) return true; str++; }`: a placeholder
        // whose number wrapped negative is passed over with the byte after it.
        let mut at = 0;
        while text.get(at).is_some() {
            match parse_arg(text, at) {
                Some((index, _)) if index >= 0 => return true,
                Some((_, next)) => at = next.saturating_add(1),
                None => at = at.saturating_add(1),
            }
        }
        false
    }

    /// `PF_VarString (first)` (RRE-1.5) of the string parameters `parms`, the call's from
    /// parameter 0: the first from `first` localized, `LOC_Format` of it when it has
    /// placeholders, else every one from `first` localized and appended, cut to 1023 bytes.
    #[must_use]
    pub fn var_string(&self, parms: &[Vec<u8>], first: usize) -> Vec<u8> {
        let Some(format) = parms.get(first) else {
            return Vec::new();
        };
        let format = self.get(format);
        let full = RRE_VARSTRING_LEN.saturating_sub(1);
        if self.has_placeholders(format) {
            let offset = first.saturating_add(1);
            let arg = |n: i32| -> &[u8] {
                let index = i64::from(n).saturating_add(i64::try_from(offset).unwrap_or(i64::MAX));
                usize::try_from(index)
                    .ok()
                    .and_then(|index| parms.get(index))
                    .map_or(&b""[..], |parm| self.get(parm))
            };
            return format_args(format, &arg, full);
        }
        // `q_strlcat` into the buffer: once it is full the rest adds nothing, and
        // `PF_VarString` returns ("overflow (string truncated)").
        let mut out = Vec::new();
        for parm in parms.get(first..).unwrap_or_default() {
            let part = self.get(parm);
            let room = full.saturating_sub(out.len());
            out.extend_from_slice(part.get(..part.len().min(room)).unwrap_or_default());
        }
        out
    }
}

/// `LOC_ParseArg` at byte `at` of `text`: the index of a `{}` (0) or `{N}` placeholder there
/// and the byte after it, the decimal `N` wrapping as x86's `int` does.
fn parse_arg(text: &[u8], at: usize) -> Option<(i32, usize)> {
    let byte = |i: usize| text.get(i).copied().unwrap_or(0);
    if byte(at) != b'{' {
        return None;
    }
    let mut i = at.saturating_add(1);
    let mut arg: i32 = 0;
    while byte(i).is_ascii_digit() {
        arg = arg
            .wrapping_mul(10)
            .wrapping_add(i32::from(byte(i).wrapping_sub(b'0')));
        i = i.saturating_add(1);
    }
    if byte(i) != b'}' {
        return None;
    }
    Some((arg, i.saturating_add(1)))
}

/// `LOC_Format (format, getarg, out, len)` (RRE-1.4): the format with each placeholder replaced
/// by its argument, at most `full` bytes (`len - 1`), an argument that does not fit cut.
fn format_args<'a>(format: &[u8], arg: &dyn Fn(i32) -> &'a [u8], full: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 0;
    while format.get(at).is_some() && out.len() < full {
        match parse_arg(format, at) {
            Some((index, next)) if index >= 0 => {
                let insert = c_string(arg(index), 0);
                let room = full.saturating_sub(out.len());
                out.extend_from_slice(insert.get(..insert.len().min(room)).unwrap_or_default());
                at = next;
            }
            // `argindex < 0`: the byte at the format's pointer is copied, past a placeholder
            // whose number wrapped negative; at the format's end there is none (C would read
            // past the terminator), so the copy ends.
            parsed => {
                let from = parsed.map_or(at, |(_, next)| next);
                let Some(&byte) = format.get(from) else {
                    break;
                };
                out.push(byte);
                at = from.saturating_add(1);
            }
        }
    }
    out
}

/// The table slot a hash starts probing at: `hash % numindices`.
fn slot(hash: u32, size: usize) -> usize {
    let size32 = u32::try_from(size).unwrap_or(u32::MAX);
    hash.checked_rem(size32)
        .and_then(|pos| usize::try_from(pos).ok())
        .unwrap_or(0)
}

/// Writes `byte` at `at` of `text`, as the in-place parse writes.
fn set(text: &mut [u8], at: usize, byte: u8) {
    if let Some(cell) = text.get_mut(at) {
        *cell = byte;
    }
}

#[cfg(test)]
mod tests;
