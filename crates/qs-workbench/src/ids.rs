// SPDX-License-Identifier: GPL-3.0-or-later

//! The numbers the workbench names things by, each its own type (CONV-17): an edict of the
//! running game, a place in a list, the host's id of a breakpoint, a checkpoint's number. Each
//! is written to JSON as the bare number, so commands take and answer what they did.

use core::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Declares a number newtype: private field, `new` and `get`, shown as the number.
macro_rules! id_newtype {
    ($(#[$meta:meta])* $name:ident($inner:ty)) => {
        $(#[$meta])*
        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
            JsonSchema,
        )]
        #[serde(transparent)]
        pub struct $name($inner);

        impl $name {
            /// The one numbered `value`.
            #[must_use]
            pub const fn new(value: $inner) -> Self {
                Self(value)
            }

            /// Its number.
            #[must_use]
            pub const fn get(self) -> $inner {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

id_newtype!(
    /// An edict of the game the preview host runs, by its number (EDICT-1.4).
    Edict(u32)
);

impl From<Edict> for qc_edict::EdictNumber {
    /// The edict as the VM numbers it; a `u32` fits a `usize` on every target the app builds
    /// for.
    fn from(edict: Edict) -> Self {
        Self::new(usize::try_from(edict.0).unwrap_or(usize::MAX))
    }
}

id_newtype!(
    /// A watch expression, by its place in the watch list.
    WatchIndex(usize)
);

id_newtype!(
    /// A code action, by its place in `code.actions`' list at the same place.
    ActionIndex(usize)
);

id_newtype!(
    /// A terminal tab, by its place among the tabs.
    TerminalIndex(usize)
);

pub use qs_engine::BreakpointId;

pub use qs_checkpoint::CheckpointNumber;
