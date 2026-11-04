// SPDX-License-Identifier: GPL-3.0-or-later

//! The back end's passes over the intermediate layer, as each dialect's profile chooses them
//! (`docs/design/back-end.md`, ADR-0020): how a body's temporaries are allocated, and how its
//! locals and temporaries are laid out among the globals. A pass another compiler needs, an
//! FTEQCC-exact allocation that reuses temporaries or GMQCC's layout, is another variant here.

use qc_syntax::Dialect;

/// How each value a body's statements make is given its slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Allocation {
    /// A new slot for each, never reused, in the order the statements make them (QC-21.5).
    CreationOrder,
}

/// Where a body's parameters, locals and temporaries are placed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Layout {
    /// Among the globals, each taking the next as it is met, as id's `qcc` places them
    /// (QC-21.4).
    InPlace,
    /// In a block of the function's own, laid out after every global, static and immediate of
    /// the module, as FTEQCC's `QCC_UnmarshalLocals` places them (FTE-21.7).
    Blocks,
}

/// The passes of one dialect's profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BackEnd {
    /// The allocation pass.
    pub(crate) allocation: Allocation,
    /// The layout pass.
    pub(crate) layout: Layout,
}

impl BackEnd {
    /// The passes `dialect`'s profile chooses: id's layout for vanilla, FTEQCC's blocks for its
    /// dialects, and the allocation by creation order for each.
    pub(crate) const fn of(dialect: Dialect) -> Self {
        let layout = if dialect.has_fteqcc_extensions() {
            Layout::Blocks
        } else {
            Layout::InPlace
        };
        Self {
            allocation: Allocation::CreationOrder,
            layout,
        }
    }
}

#[cfg(test)]
mod tests {
    use qc_syntax::Dialect;

    use super::{Allocation, BackEnd, Layout};

    // spec QC-21.4, QC-21.5, FTE-21.7: vanilla lays a body's locals out among the globals as
    // they are met, FTEQCC in a block of the function's own; each allocates by creation order
    #[test]
    fn each_profile_chooses_its_passes() {
        assert_eq!(
            BackEnd::of(Dialect::Vanilla),
            BackEnd {
                allocation: Allocation::CreationOrder,
                layout: Layout::InPlace,
            }
        );
        assert_eq!(
            BackEnd::of(Dialect::Fteqcc),
            BackEnd {
                allocation: Allocation::CreationOrder,
                layout: Layout::Blocks,
            }
        );
    }
}
