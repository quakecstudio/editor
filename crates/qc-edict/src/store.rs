// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_edict.c, ED_Alloc, ED_Free, ED_ClearEdict, EDICT_NUM, NUM_FOR_EDICT
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/sv_main.c, SV_SpawnServer

//! The edict array and the allocation, freeing and field access over it.
//!
//! Implements EDICT-1.4 (the array of [`LIMIT_MAX_EDICTS`] edicts and `num_edicts`), EDICT-1.7
//! (turning a number into an edict and back), EDICT-6 (`spawn`, id's `ED_Alloc`) and EDICT-7
//! (`remove`, id's `ED_Free`). The free flag and `freetime` are the header fields the store
//! models (EDICT-1.2); the rest of the header is not addressable by QuakeC.

use core::fmt;

use qc_progs::limits::LIMIT_MAX_EDICTS;
use qc_types::{EntityRef, Slot};

use crate::EdictNumber;
use crate::system::SysField;
use crate::{EDICT_FREED_NEXTTHINK, EDICT_REUSE_DELAY, EDICT_REUSE_GRACE, EdictLayout};

/// The field slots id's `ED_Free` sets to 0 (EDICT-7.1), by their slots from EDICT-3.1.
///
/// `model` (29), `takedamage` (59), `modelindex` (0), `colormap` (77), `skin` (31),
/// `frame` (30) and `solid` (9) are scalars; `origin` (10, 11, 12) and `angles` (19, 20, 21)
/// are vectors, each cleared in full. `nextthink` (46) goes to [`EDICT_FREED_NEXTTHINK`]
/// instead. `absmin` and `absmax` are not cleared. From id's `ED_Free` in
/// `WinQuake/pr_edict.c`.
const FREE_ZEROED: [SysField; 9] = [
    SysField::Model,
    SysField::Takedamage,
    SysField::Modelindex,
    SysField::Colormap,
    SysField::Skin,
    SysField::Frame,
    SysField::Solid,
    SysField::Origin,
    SysField::Angles,
];

/// The `nextthink` field slot (EDICT-3.1), which `ED_Free` sets to -1, not 0.
const FREE_NEXTTHINK_SLOT: usize = SysField::Nextthink.slot();

/// One edict: the header fields the store models and its field slots.
///
/// EDICT-1.2: the free flag and `freetime` are the only header fields QuakeC reads, through
/// allocation (EDICT-6) and `nextent` (EDICT-5.4); the rest of the header is engine state the
/// store does not model. The `fields` are the `entityfields` 4-byte slots (EDICT-1.1).
#[derive(Clone, Debug, PartialEq)]
struct Edict {
    /// id's `edict_t.free`: whether the edict is free to be reused (EDICT-6.1).
    free: bool,
    /// id's `edict_t.freetime`: the server time the edict was last freed, rounded to `f32`
    /// (EDICT-1.2, EDICT-7.1).
    freetime: f32,
    /// The `entityfields` field slots, id's `edict_t.v` (EDICT-1.1).
    fields: Box<[Slot]>,
}

impl Edict {
    /// A zero-filled, in-use edict, as `SV_SpawnServer` allocates one (EDICT-1.8).
    fn zeroed(entityfields: usize) -> Self {
        Self {
            free: false,
            freetime: 0.0,
            fields: vec![Slot::ZERO; entityfields].into_boxed_slice(),
        }
    }
}

/// Turning an entity value into an edict failed.
///
/// Implements EDICT-1.7: id's `EDICT_NUM` and `NUM_FOR_EDICT` call `Sys_Error` with these
/// messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EdictError {
    /// id's `EDICT_NUM(n)`: `n` is negative or not less than [`LIMIT_MAX_EDICTS`].
    BadNumber {
        /// The number asked for.
        number: i32,
    },
    /// id's `NUM_FOR_EDICT`: the value is negative or names an edict at or above `num_edicts`.
    BadPointer,
    /// id's `ED_Alloc`: `num_edicts` is already [`LIMIT_MAX_EDICTS`] and nothing qualifies.
    NoFreeEdicts,
    /// `maxclients` is not 1 to 16, so the client slots do not fit the array (EDICT-5.1).
    BadMaxclients {
        /// The count asked for.
        maxclients: usize,
    },
    /// A field write names an edict the array does not hold, or a slot past its fields.
    NoField {
        /// The edict.
        number: EdictNumber,
        /// The field slot.
        field: usize,
    },
}

impl fmt::Display for EdictError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadNumber { number } => write!(f, "EDICT_NUM: bad number {number}"),
            Self::BadPointer => f.write_str("NUM_FOR_EDICT: bad pointer"),
            Self::NoFreeEdicts => f.write_str("ED_Alloc: no free edicts"),
            Self::BadMaxclients { maxclients } => {
                write!(f, "maxclients {maxclients} is not 0 to 16")
            }
            Self::NoField { number, field } => {
                write!(f, "edict {number} has no field slot {field}")
            }
        }
    }
}

impl core::error::Error for EdictError {}

/// The server's edict array for one map.
///
/// Implements EDICT-1.4: [`LIMIT_MAX_EDICTS`] edicts, numbered from 0, with `num_edicts`
/// starting at `maxclients + 1` and growing by one on each new allocation (EDICT-6.2). Edict 0
/// is the world and edicts 1 to `maxclients` are the client slots (EDICT-5.1). Every edict
/// starts zero-filled and in use (EDICT-1.8).
#[derive(Clone, Debug)]
pub struct EdictStore {
    layout: EdictLayout,
    edicts: Vec<Edict>,
    num_edicts: usize,
    maxclients: usize,
}

impl EdictStore {
    /// A fresh store for a map, id's `SV_SpawnServer` after `PR_LoadProgs` (EDICT-1.8).
    ///
    /// Allocates all [`LIMIT_MAX_EDICTS`] edicts zero-filled and in use, and sets `num_edicts`
    /// to `maxclients + 1` (EDICT-1.8). Edict 0 is the world and edicts 1 to `maxclients` are
    /// the client slots (EDICT-5.1).
    ///
    /// A real server has 1 to 16 clients (EDICT-5.1); 0 is allowed for the VM test
    /// environment, which has no client slots.
    ///
    /// # Errors
    ///
    /// [`EdictError::BadMaxclients`] when `maxclients` is more than
    /// [`EDICT_MAX_CLIENTS`](crate::EDICT_MAX_CLIENTS).
    pub fn new(layout: EdictLayout, maxclients: usize) -> Result<Self, EdictError> {
        if maxclients > crate::EDICT_MAX_CLIENTS {
            return Err(EdictError::BadMaxclients { maxclients });
        }
        let entityfields = layout.entityfields();
        let edicts = vec![Edict::zeroed(entityfields); LIMIT_MAX_EDICTS];
        let num_edicts = maxclients.saturating_add(1);
        Ok(Self {
            layout,
            edicts,
            num_edicts,
            maxclients,
        })
    }

    /// The layout of this store's edicts.
    #[must_use]
    pub const fn layout(&self) -> &EdictLayout {
        &self.layout
    }

    /// The number of edicts in use, id's `sv.num_edicts` (EDICT-1.4).
    ///
    /// Edicts at or above it have not been used on this map; it never shrinks (EDICT-7.3).
    #[must_use]
    pub const fn num_edicts(&self) -> usize {
        self.num_edicts
    }

    /// The client slot count, id's `svs.maxclients` (EDICT-5.1).
    #[must_use]
    pub const fn maxclients(&self) -> usize {
        self.maxclients
    }

    /// Whether edict `number` is a client's: 1 to [`maxclients`](Self::maxclients)
    /// (EDICT-5.1), the range id's builtins test as `entnum < 1 || entnum > svs.maxclients`.
    #[must_use]
    pub const fn is_client(&self, number: EdictNumber) -> bool {
        number.index() >= 1 && number.index() <= self.maxclients
    }

    /// Whether edict `number` is free, id's `edict_t.free`.
    ///
    /// Implements EDICT-6.1. Returns `None` for a number the array does not hold.
    #[must_use]
    pub fn is_free(&self, number: EdictNumber) -> Option<bool> {
        self.edicts.get(number.index()).map(|edict| edict.free)
    }

    /// The server time edict `number` was last freed, id's `edict_t.freetime` (EDICT-1.2).
    ///
    /// Returns `None` for a number the array does not hold.
    #[must_use]
    pub fn freetime(&self, number: EdictNumber) -> Option<f32> {
        self.edicts.get(number.index()).map(|edict| edict.freetime)
    }

    /// Edict `number`'s field slots, id's `edict_t.v`.
    ///
    /// Returns `None` for a number the array does not hold (EDICT-1.4). The slots are returned
    /// whether or not the edict is in use or free, since the array holds every edict
    /// (EDICT-1.9, EDICT-7.2).
    #[must_use]
    pub fn fields(&self, number: EdictNumber) -> Option<&[Slot]> {
        self.edicts.get(number.index()).map(|edict| &*edict.fields)
    }

    /// Edict `number`'s field slots for writing.
    ///
    /// Returns `None` for a number the array does not hold.
    #[must_use]
    pub fn fields_mut(&mut self, number: EdictNumber) -> Option<&mut [Slot]> {
        self.edicts
            .get_mut(number.index())
            .map(|edict| &mut *edict.fields)
    }

    /// Field slot `field` of edict `number`, read by offset.
    ///
    /// Implements the field access of EDICT-1.6: a field is addressed by its slot offset.
    /// Returns `None` when the edict or the slot does not exist.
    #[must_use]
    pub fn field(&self, number: EdictNumber, field: usize) -> Option<Slot> {
        self.edicts
            .get(number.index())
            .and_then(|edict| edict.fields.get(field).copied())
    }

    /// Sets field slot `field` of edict `number`, written by offset.
    ///
    /// Implements the field access of EDICT-1.6.
    ///
    /// # Errors
    ///
    /// [`EdictError::NoField`], writing nothing, when the edict or the slot does not exist.
    pub fn set_field(
        &mut self,
        number: EdictNumber,
        field: usize,
        value: Slot,
    ) -> Result<(), EdictError> {
        let slot = self
            .edicts
            .get_mut(number.index())
            .and_then(|edict| edict.fields.get_mut(field))
            .ok_or(EdictError::NoField { number, field })?;
        *slot = value;
        Ok(())
    }

    /// Turns an edict number into its entity value, id's `EDICT_NUM` then `EDICT_TO_PROG`.
    ///
    /// Implements EDICT-1.7: fails with [`EdictError::BadNumber`] when `number` is negative or
    /// not less than [`LIMIT_MAX_EDICTS`].
    ///
    /// # Errors
    ///
    /// [`EdictError::BadNumber`] for a number outside 0 to [`LIMIT_MAX_EDICTS`] - 1.
    pub fn edict_num(&self, number: i32) -> Result<EntityRef, EdictError> {
        let max = i32::try_from(LIMIT_MAX_EDICTS).map_err(|_| EdictError::BadNumber { number })?;
        if number < 0 || number >= max {
            return Err(EdictError::BadNumber { number });
        }
        EdictNumber::from_i32(number)
            .and_then(|edict| self.layout.entity_value(edict))
            .ok_or(EdictError::BadNumber { number })
    }

    /// Turns an entity value into its edict number, id's `NUM_FOR_EDICT`.
    ///
    /// Implements EDICT-1.7: fails with [`EdictError::BadPointer`] when the value is negative
    /// or names an edict at or above `num_edicts`. A value that names no edict (EDICT-1.9)
    /// also fails here, as id's `PROG_TO_EDICT` would give an address outside the array.
    ///
    /// # Errors
    ///
    /// [`EdictError::BadPointer`] for a value that names no edict below `num_edicts`.
    pub fn num_for_edict(&self, value: EntityRef) -> Result<EdictNumber, EdictError> {
        let number = self
            .layout
            .edict_number(value)
            .ok_or(EdictError::BadPointer)?;
        if number.index() >= self.num_edicts {
            return Err(EdictError::BadPointer);
        }
        Ok(number)
    }

    /// Sets everything in edict `number` to zero and clears its free flag, id's `ED_ClearEdict`.
    ///
    /// Implements EDICT-6.5: `memset(&e->v, 0, entityfields * 4); e->free = false`. The header
    /// is otherwise left as it was, including `freetime` (EDICT-6.5). Does nothing for a
    /// number the array does not hold.
    pub fn clear_edict(&mut self, number: EdictNumber) {
        if let Some(edict) = self.edicts.get_mut(number.index()) {
            edict.fields.fill(Slot::ZERO);
            edict.free = false;
        }
    }

    /// Allocates an edict, id's `ED_Alloc`, for the `spawn` builtin and each map entity after
    /// the first (EDICT-6).
    ///
    /// Implements EDICT-6.1 to EDICT-6.6: scans edicts `maxclients + 1` to `num_edicts - 1` in
    /// order and takes the first free one whose `freetime` is below [`EDICT_REUSE_GRACE`] or
    /// more than [`EDICT_REUSE_DELAY`] seconds before `now`; otherwise takes edict
    /// `num_edicts` and grows `num_edicts` by one. The clock `now` is `sv.time`, a `double`.
    /// Returns the new edict's number; its fields are zeroed and its free flag cleared
    /// (EDICT-6.5).
    ///
    /// # Errors
    ///
    /// [`EdictError::NoFreeEdicts`] when `num_edicts` is already [`LIMIT_MAX_EDICTS`] and no
    /// scanned edict qualifies (EDICT-6.2).
    pub fn alloc(&mut self, now: f64) -> Result<EdictNumber, EdictError> {
        // EDICT-6.1: for (i = maxclients + 1; i < num_edicts; i++).
        let start = self.maxclients.saturating_add(1);
        for number in (start..self.num_edicts).map(EdictNumber::new) {
            let qualifies = self.edicts.get(number.index()).is_some_and(|edict| {
                // EDICT-6.3: e->freetime < 2 || sv.time - e->freetime > 0.5, the delay in
                // double precision against the f32 freetime, strict.
                // EDICT-6.1: free and old enough to reuse.
                edict.free
                    && (edict.freetime < EDICT_REUSE_GRACE
                        || now - f64::from(edict.freetime) > EDICT_REUSE_DELAY)
            });
            if qualifies {
                self.clear_edict(number);
                return Ok(number);
            }
        }
        // EDICT-6.2: if (i == MAX_EDICTS) Sys_Error("ED_Alloc: no free edicts").
        if self.num_edicts >= LIMIT_MAX_EDICTS {
            return Err(EdictError::NoFreeEdicts);
        }
        let number = EdictNumber::new(self.num_edicts);
        self.num_edicts = self.num_edicts.saturating_add(1);
        self.clear_edict(number);
        Ok(number)
    }

    /// Frees edict `number`, id's `ED_Free`, for the `remove` builtin (EDICT-7).
    ///
    /// Implements EDICT-7.1, EDICT-7.4: sets the free flag, sets `freetime` to `now` rounded
    /// to `f32`, zeroes `model`, `takedamage`, `modelindex`, `colormap`, `skin`, `frame`,
    /// `origin`, `angles` and `solid`, and sets `nextthink` to [`EDICT_FREED_NEXTTHINK`].
    /// Every other field keeps its value (EDICT-7.2). Freeing an already-free edict does it
    /// all again (EDICT-7.4). The clock `now` is `sv.time`, a `double`. Does nothing for a
    /// number the array does not hold.
    pub fn free(&mut self, number: EdictNumber, now: f64) {
        let Some(edict) = self.edicts.get_mut(number.index()) else {
            return;
        };
        edict.free = true;
        // EDICT-7.1: ed->v.model = 0, takedamage = 0, modelindex = 0, colormap = 0, skin = 0,
        // frame = 0, solid = 0, origin = 0 0 0, angles = 0 0 0, nextthink = -1.
        for field in FREE_ZEROED {
            let var = field.var();
            for slot in var.slot..var.slot.saturating_add(var.etype.slot_count()) {
                if let Some(cell) = edict.fields.get_mut(slot) {
                    *cell = Slot::ZERO;
                }
            }
        }
        // nextthink to -1, not 0.
        if let Some(cell) = edict.fields.get_mut(FREE_NEXTTHINK_SLOT) {
            *cell = Slot::from_f32(EDICT_FREED_NEXTTHINK);
        }
        // EDICT-7.1: ed->freetime = sv.time, rounded to f32 by the store.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "spec EDICT-7.1: freetime is an f32, so storing the double clock rounds it"
        )]
        {
            edict.freetime = now as f32;
        }
    }
}
