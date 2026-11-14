// SPDX-License-Identifier: GPL-3.0-or-later

//! Where a value lives in the progs' memory: a global's offset, `ofs` in the globals, and a
//! field's, `ofs` in each edict's fields (PROGS-17, EDICT-1).

/// A global's offset in the globals, `G_INT(ofs)`'s `ofs`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GlobalOffset(usize);

impl GlobalOffset {
    /// Offset `offset`.
    #[must_use]
    pub const fn new(offset: usize) -> Self {
        Self(offset)
    }

    /// The offset.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}

/// A field's offset in an edict's fields, a field definition's `ofs`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FieldOffset(usize);

impl FieldOffset {
    /// Offset `offset`.
    #[must_use]
    pub const fn new(offset: usize) -> Self {
        Self(offset)
    }

    /// The offset.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}
