// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_edict.c, PR_LoadProgs, EDICT_NUM, NUM_FOR_EDICT
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/progs.h, EDICT_TO_PROG, PROG_TO_EDICT, NEXT_EDICT

//! How the edicts are laid out for one loaded progs, and the numbering QuakeC addresses them
//! by.
//!
//! Implements EDICT-1. The layout fixes the edict size from the progs' field count
//! (EDICT-1.1) and the 32-bit header size (EDICT-1.3), turns an edict number into its entity
//! value (EDICT-1.5, id's `EDICT_TO_PROG`) and a field slot into a pointer value (EDICT-1.6,
//! id's `OP_ADDRESS`), and refuses a progs that cannot hold the system globals or fields
//! (EDICT-2.4, EDICT-3.3) or whose field count would overflow an entity value (EDICT-3.5).

use core::fmt;

use qc_progs::Progs;
use qc_progs::limits::LIMIT_MAX_EDICTS;
use qc_types::{EntityRef, PointerRef};

use crate::{
    EDICT_HEADER_SIZE, EDICT_MAX_FIELDS, EDICT_SYS_FIELDS_END, EDICT_SYS_GLOBALS_END, EdictNumber,
};

/// Why a progs cannot back an edict store.
///
/// The engine would read or write past the end of the globals or the edicts, or overflow an
/// `int` (EDICT-2.4, EDICT-3.3, EDICT-3.5); this spec refuses the progs with a typed error
/// when the store is set up instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LayoutError {
    /// The progs has fewer than [`EDICT_SYS_GLOBALS_END`] global slots (EDICT-2.4).
    GlobalsTooFew {
        /// The progs' global slot count.
        globals: usize,
    },
    /// The progs' `entityfields` is below [`EDICT_SYS_FIELDS_END`] (EDICT-3.3).
    FieldsTooFew {
        /// The progs' field slot count.
        fields: usize,
    },
    /// The progs' `entityfields` is above [`EDICT_MAX_FIELDS`] (EDICT-3.5).
    FieldsTooMany {
        /// The progs' field slot count.
        fields: usize,
    },
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GlobalsTooFew { globals } => write!(
                f,
                "progs.dat has {globals} global slots, fewer than the {EDICT_SYS_GLOBALS_END} \
                 the system globals need"
            ),
            Self::FieldsTooFew { fields } => write!(
                f,
                "progs.dat has {fields} field slots, fewer than the {EDICT_SYS_FIELDS_END} the \
                 system fields need"
            ),
            Self::FieldsTooMany { fields } => write!(
                f,
                "progs.dat has {fields} field slots, more than the {EDICT_MAX_FIELDS} an entity \
                 value can address"
            ),
        }
    }
}

impl core::error::Error for LayoutError {}

/// The layout of the edict array for one loaded progs.
///
/// Implements EDICT-1.1: an edict is [`EDICT_HEADER_SIZE`] bytes of header followed by
/// `entityfields` 4-byte field slots. This is a plain value; [`EdictStore`](crate::EdictStore)
/// holds the edicts themselves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EdictLayout {
    /// The progs' `entityfields`: 4-byte field slots per edict.
    entityfields: usize,
    /// The edict size in bytes, `96 + 4 * entityfields` (EDICT-1.1).
    edict_size: i32,
}

impl EdictLayout {
    /// The layout for a progs with `entityfields` field slots and `numglobals` global slots.
    ///
    /// Implements EDICT-1.1, EDICT-2.4, EDICT-3.3 and EDICT-3.5.
    ///
    /// # Errors
    ///
    /// [`LayoutError`] when the progs cannot hold the system globals or fields, or its field
    /// count would overflow an entity value.
    pub fn new(entityfields: usize, numglobals: usize) -> Result<Self, LayoutError> {
        if numglobals < EDICT_SYS_GLOBALS_END {
            return Err(LayoutError::GlobalsTooFew {
                globals: numglobals,
            });
        }
        if entityfields < EDICT_SYS_FIELDS_END {
            return Err(LayoutError::FieldsTooFew {
                fields: entityfields,
            });
        }
        if entityfields > EDICT_MAX_FIELDS {
            return Err(LayoutError::FieldsTooMany {
                fields: entityfields,
            });
        }
        // EDICT-1.1: 96 + 4 * entityfields. entityfields is at most EDICT_MAX_FIELDS, so this
        // is at most 2147483583, which fits an i32; both operations are still checked.
        let field_bytes = i32::try_from(entityfields)
            .ok()
            .and_then(|fields| fields.checked_mul(4))
            .and_then(|bytes| bytes.checked_add(EDICT_HEADER_SIZE));
        let Some(edict_size) = field_bytes else {
            return Err(LayoutError::FieldsTooMany {
                fields: entityfields,
            });
        };
        Ok(Self {
            entityfields,
            edict_size,
        })
    }

    /// The layout for a loaded progs, id's `PR_LoadProgs` setting `pr_edict_size`.
    ///
    /// Implements EDICT-1.1 from the progs' `entityfields` and global slot count.
    ///
    /// # Errors
    ///
    /// [`LayoutError`] as for [`EdictLayout::new`].
    pub fn for_progs(progs: &Progs<'_>) -> Result<Self, LayoutError> {
        let header = progs.header();
        let entityfields = usize::try_from(header.entityfields)
            .map_err(|_| LayoutError::FieldsTooFew { fields: 0 })?;
        let numglobals = usize::try_from(header.numglobals)
            .map_err(|_| LayoutError::GlobalsTooFew { globals: 0 })?;
        Self::new(entityfields, numglobals)
    }

    /// The field slots each edict has, id's `entityfields`.
    #[must_use]
    pub const fn entityfields(&self) -> usize {
        self.entityfields
    }

    /// The edict size in bytes, id's `pr_edict_size` (EDICT-1.1).
    #[must_use]
    pub const fn edict_size(&self) -> i32 {
        self.edict_size
    }

    /// The entity value of edict `number`, id's `EDICT_TO_PROG` (EDICT-1.5).
    ///
    /// The value is the number times the edict size; edict 0 is the world, value 0. Returns
    /// `None` only when the product overflows an `i32`, which no valid edict number does.
    #[must_use]
    pub fn entity_value(&self, number: EdictNumber) -> Option<EntityRef> {
        EntityRef::from_number(number.to_i32()?, self.edict_size)
    }

    /// The edict number an entity value names, if it names one, id's `PROG_TO_EDICT` read back
    /// through `NUM_FOR_EDICT`.
    ///
    /// Implements EDICT-1.9: a value names edict `n` when it is `n` times the edict size for a
    /// whole number `n` from 0 to [`LIMIT_MAX_EDICTS`](qc_progs::limits::LIMIT_MAX_EDICTS) - 1;
    /// any other value names none. It does not check whether edict `n` is in use or free: the
    /// array holds all the edicts.
    #[must_use]
    pub fn edict_number(&self, value: EntityRef) -> Option<EdictNumber> {
        let number = value.number(self.edict_size)?;
        let max = i32::try_from(LIMIT_MAX_EDICTS).ok()?;
        if number >= 0 && number < max {
            EdictNumber::from_i32(number)
        } else {
            None
        }
    }

    /// The pointer value for field slot `field` of edict `number`, id's `OP_ADDRESS`
    /// (EDICT-1.6).
    ///
    /// The value is `number * edict_size + 96 + 4 * field`. Returns `None` when the slot is
    /// not below [`entityfields`](Self::entityfields) or the arithmetic overflows an `i32`.
    #[must_use]
    pub fn pointer_value(&self, number: EdictNumber, field: usize) -> Option<PointerRef> {
        if field >= self.entityfields {
            return None;
        }
        let field = i32::try_from(field).ok()?;
        let base = number.to_i32()?.checked_mul(self.edict_size)?;
        let offset = field
            .checked_mul(4)
            .and_then(|bytes| bytes.checked_add(EDICT_HEADER_SIZE))
            .and_then(|header_and_field| base.checked_add(header_and_field))?;
        Some(PointerRef::from_bits(offset))
    }

    /// The edict number and field slot `pointer` names, the inverse of
    /// [`pointer_value`](Self::pointer_value) (EDICT-1.6): `None` for a negative value, one
    /// inside an edict's header, one not on a field slot's 4-byte boundary, or a slot not below
    /// [`entityfields`](Self::entityfields). Whether the edict exists is the store's to say.
    #[must_use]
    pub fn pointer_slot(&self, pointer: PointerRef) -> Option<(EdictNumber, usize)> {
        let value = pointer.bits();
        let number = value.checked_div(self.edict_size)?;
        // A negative value has a remainder of 0 or less, so it lands in the header below.
        let within = value.checked_rem(self.edict_size)?;
        let from_fields = within.checked_sub(EDICT_HEADER_SIZE)?;
        if from_fields < 0 || from_fields.checked_rem(4) != Some(0) {
            return None;
        }
        // The edict size is exactly the header and `entityfields` slots (EDICT-1.1), so a
        // remainder below it is always a slot below `entityfields`.
        let field = usize::try_from(from_fields.checked_div(4)?).ok()?;
        Some((EdictNumber::from_i32(number)?, field))
    }
}
