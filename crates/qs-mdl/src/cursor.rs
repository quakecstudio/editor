// SPDX-License-Identifier: GPL-3.0-or-later

//! The little-endian reads and the file cursor the alias model and sprite readers share.

/// The `N` bytes at `offset`, or `None` past the end of `bytes`.
pub(crate) fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Option<[u8; N]> {
    bytes.get(offset..offset.checked_add(N)?)?.try_into().ok()
}

/// The signed little-endian 32-bit integer at `offset`, or `None` past the end of `bytes`.
pub(crate) fn le_i32(bytes: &[u8], offset: usize) -> Option<i32> {
    read_array(bytes, offset).map(i32::from_le_bytes)
}

/// A cursor over a model file that fails at the end with the error `truncated` makes for the
/// offset it was reading (MDL-2.5, MDL-5.6, SPR-2.4).
pub(crate) struct Cursor<'a, E> {
    bytes: &'a [u8],
    pos: usize,
    truncated: fn(usize) -> E,
}

impl<'a, E> Cursor<'a, E> {
    /// A cursor over `bytes` at `pos`.
    pub(crate) const fn new(bytes: &'a [u8], pos: usize, truncated: fn(usize) -> E) -> Self {
        Self {
            bytes,
            pos,
            truncated,
        }
    }

    /// Where it reads next.
    pub(crate) const fn pos(&self) -> usize {
        self.pos
    }

    /// The next `len` bytes.
    pub(crate) fn take(&mut self, len: usize) -> Result<&'a [u8], E> {
        let end = self.pos.checked_add(len);
        let slice = end.and_then(|end| self.bytes.get(self.pos..end));
        match (slice, end) {
            (Some(slice), Some(end)) => {
                self.pos = end;
                Ok(slice)
            }
            _ => Err((self.truncated)(self.pos)),
        }
    }

    /// The next little-endian 32-bit integer.
    pub(crate) fn int(&mut self) -> Result<i32, E> {
        let pos = self.pos;
        let word = self.take(4)?;
        le_i32(word, 0).ok_or_else(|| (self.truncated)(pos))
    }

    /// The next 32-bit float.
    pub(crate) fn float(&mut self) -> Result<f32, E> {
        Ok(f32::from_bits(self.int()?.cast_unsigned()))
    }

    /// The `count` intervals of a group, each refused with `bad` when it is 0 or less. id's
    /// test is `interval <= 0`, so a NaN passes (MDL-2.3, MDL-5.5, SPR-2.6).
    pub(crate) fn intervals(
        &mut self,
        count: usize,
        bad: impl Fn(f32) -> E,
    ) -> Result<Vec<f32>, E> {
        let mut intervals = Vec::new();
        for _ in 0..count {
            let interval = self.float()?;
            if interval <= 0.0 {
                return Err(bad(interval));
            }
            intervals.push(interval);
        }
        Ok(intervals)
    }
}
