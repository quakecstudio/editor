// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2016      Spike
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from Novum/vkQuake@27789d7333a46674474d871137bb71e056719628: Quake/pr_ext.c, PF_buf_create, PF_buf_del, PF_buf_getsize, PF_bufstr_get, PF_bufstr_add, PF_bufstr_add_internal

//! vkQuake's string buffers (BUILTIN-18.39): 64 buffers, each a list of strings or holes.

/// `NUMSTRINGBUFS`, how many buffers there are.
const NUMSTRINGBUFS: usize = 64;
/// vkQuake's `STRINGTEMP_LENGTH`, the most bytes `bufstr_get` returns, its zero among them.
const STRINGTEMP_LENGTH: usize = 1024;

/// The 64 buffers, `strbuflist`: a buffer in use holds its strings, a slot of which may be a
/// hole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StringBuffers {
    buffers: Vec<Option<Vec<Option<Vec<u8>>>>>,
}

impl Default for StringBuffers {
    fn default() -> Self {
        Self {
            buffers: vec![None; NUMSTRINGBUFS],
        }
    }
}

impl StringBuffers {
    /// `PF_buf_create`: the handle, from 1 (`BUFSTRBASE`), of the first free buffer, now in use
    /// and empty; -1 when `string_type` is not `string` in any case or every buffer is in use.
    pub fn create(&mut self, string_type: &[u8]) -> f32 {
        if !string_type.eq_ignore_ascii_case(b"string") {
            return -1.0;
        }
        let Some(index) = self.buffers.iter().position(Option::is_none) else {
            return -1.0;
        };
        if let Some(slot) = self.buffers.get_mut(index) {
            *slot = Some(Vec::new());
        }
        f32::from(u8::try_from(index.saturating_add(1)).unwrap_or(u8::MAX))
    }

    /// The buffer of slot `slot`, when it is one in use.
    fn buffer(&mut self, slot: u32) -> Option<&mut Vec<Option<Vec<u8>>>> {
        self.buffers.get_mut(usize::try_from(slot).ok()?)?.as_mut()
    }

    /// `PF_buf_del`: buffer slot `slot` no longer in use, its strings gone.
    pub fn delete(&mut self, slot: u32) {
        if let Some(buffer) = usize::try_from(slot)
            .ok()
            .and_then(|n| self.buffers.get_mut(n))
        {
            *buffer = None;
        }
    }

    /// `PF_buf_getsize`: how many slots buffer `slot` uses, holes among them, or `None` for
    /// a buffer not in use, when vkQuake leaves the return value as it was.
    pub fn size(&mut self, slot: u32) -> Option<usize> {
        self.buffer(slot).map(|buffer| buffer.len())
    }

    /// `PF_bufstr_get`: string `index` of buffer `slot`, at most 1023 bytes of it, or `None`
    /// for a hole, an index past the end or a buffer not in use.
    pub fn get(&mut self, slot: u32, index: u32) -> Option<Vec<u8>> {
        let buffer = self.buffer(slot)?;
        let mut text = buffer.get(usize::try_from(index).ok()?)?.clone()?;
        text.truncate(STRINGTEMP_LENGTH.saturating_sub(1));
        Some(text)
    }

    /// `PF_bufstr_add`: `text` put at the end of buffer `slot` when `at_end`, else in its first
    /// hole or at its end; the index, or `None` for a buffer not in use, when vkQuake leaves the
    /// return value as it was.
    pub fn add(&mut self, slot: u32, text: &[u8], at_end: bool) -> Option<usize> {
        let buffer = self.buffer(slot)?;
        let index = if at_end {
            buffer.len()
        } else {
            buffer
                .iter()
                .position(Option::is_none)
                .unwrap_or(buffer.len())
        };
        if index >= buffer.len() {
            buffer.resize(index.saturating_add(1), None);
        }
        if let Some(place) = buffer.get_mut(index) {
            *place = Some(text.to_vec());
        }
        Some(index)
    }
}

#[cfg(test)]
mod tests;
