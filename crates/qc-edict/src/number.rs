// SPDX-License-Identifier: GPL-3.0-or-later

//! The number of an edict.

use core::fmt;

/// An edict's number: its index in the server's edict array, id's `NUM_FOR_EDICT` (EDICT-1.4).
/// Edict 0 is the world and edicts 1 to `maxclients` the clients (EDICT-5.1). A QuakeC value
/// names an edict through its entity value, not its number (EDICT-1.5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EdictNumber(usize);

impl EdictNumber {
    /// Edict 0, the world.
    pub const WORLD: Self = Self(0);

    /// Edict `index`.
    #[must_use]
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    /// Its index in the edict array.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }

    /// Edict `number`, a number QuakeC or the engine holds as an `int`; `None` when negative.
    #[must_use]
    pub fn from_i32(number: i32) -> Option<Self> {
        usize::try_from(number).ok().map(Self)
    }

    /// The number as the `int` id's code holds it; `None` past `i32::MAX`, which no edict
    /// array reaches.
    #[must_use]
    pub fn to_i32(self) -> Option<i32> {
        i32::try_from(self.0).ok()
    }
}

impl fmt::Display for EdictNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
