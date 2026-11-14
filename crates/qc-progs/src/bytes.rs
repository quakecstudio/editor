// SPDX-License-Identifier: GPL-3.0-or-later

//! Fixed-size reads from a file's bytes, which every reader of id's little-endian formats
//! (progs.dat, PAK, WAD2, BSP, MDL) shares.

/// The `N` bytes at `offset`, or `None` past the end of `bytes`.
#[must_use]
pub fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Option<[u8; N]> {
    bytes.get(offset..offset.checked_add(N)?)?.try_into().ok()
}
