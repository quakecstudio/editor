// SPDX-License-Identifier: GPL-3.0-or-later

//! The running map's BSP file, owned with the BSP read from it, so a new level or a swap frees
//! the last one instead of leaking it (BRIDGE-4.17).

#![expect(
    clippy::mem_forget,
    reason = "self_cell's macro forgets the owner it moved into the cell; the cell drops it"
)]

use qs_bsp::{Bsp, BspError, BspLoader};

self_cell::self_cell!(
    /// A map's BSP file and the [`Bsp`] read from it, which borrows the file's bytes.
    pub(crate) struct MapBsp {
        owner: Vec<u8>,
        #[covariant]
        dependent: Bsp,
    }
);

impl MapBsp {
    /// The BSP of `bytes`, read as the engine loads a map.
    ///
    /// # Errors
    ///
    /// The [`BspError`] the file does not read with.
    pub(crate) fn read(bytes: Vec<u8>) -> Result<Self, BspError> {
        Self::try_new(bytes, |bytes| Bsp::read(bytes, BspLoader::default()))
    }

    /// The file's bytes.
    pub(crate) fn bytes(&self) -> &[u8] {
        self.borrow_owner()
    }

    /// The BSP.
    pub(crate) fn bsp(&self) -> &Bsp<'_> {
        self.borrow_dependent()
    }
}
