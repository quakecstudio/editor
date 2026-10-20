// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_edict.c, ED_Alloc, ED_Free, ED_FindField, ED_FindFunction, EDICT_NUM

//! The [`SpawnHost`] the EDICT-8 port drives when spawning a map (EDICT-8).
//!
//! [`Spawner`] connects the pure EDICT-8 port ([`crate::edict8`]) to the running [`Host`]: it
//! allocates edicts from the edict store (`ED_Alloc`, EDICT-6, and the world for the first
//! entity, EDICT-4.2), finds fields and functions in the progs (`ED_FindField`,
//! `ED_FindFunction`), reads and writes fields and globals, makes engine strings, and runs or
//! frees each entity (EDICT-8.6, EDICT-8.5). The clock stays at 1 throughout (EDICT-8.1), so a
//! freed entity is reused at once (EDICT-8.7).

use qc_progs::Progs;
use qc_types::{DefType, Slot};

use crate::edict8::{FieldDef, SPAWNFLAGS_SLOT, SpawnError, SpawnHost};

use super::Host;

/// Drives the EDICT-8 port against the running host and progs.
pub(crate) struct Spawner<'a, 'h, 'p> {
    host: &'a mut Host<'h>,
    progs: &'a Progs<'p>,
}

impl<'a, 'h, 'p> Spawner<'a, 'h, 'p> {
    /// A spawner over `host` and the loaded `progs`.
    pub(crate) fn new(host: &'a mut Host<'h>, progs: &'a Progs<'p>) -> Self {
        Self { host, progs }
    }
}

impl SpawnHost for Spawner<'_, '_, '_> {
    fn allocate(&mut self, first: bool) -> Result<usize, SpawnError> {
        if first {
            // EDICT-4.2: the map's first entity goes into edict 0 without clearing it.
            Ok(0)
        } else {
            // EDICT-6: ED_Alloc at the map-start clock (1.0, EDICT-8.1); a full array is the
            // fatal ED_Alloc: no free edicts (EDICT-6.2), the store's only alloc error.
            let clock = self.host.builtins.engine().clock;
            self.host
                .machine
                .edicts_mut()
                .alloc(clock)
                .map_err(|_| SpawnError::NoFreeEdicts)
        }
    }

    fn edict_value(&self, number: i32) -> Result<i32, SpawnError> {
        // EDICT-1.7: EDICT_NUM fails for a number below 0 or from LIMIT_MAX_EDICTS on.
        self.host
            .machine
            .edicts()
            .edict_num(number)
            .map(|entity| entity.0)
            .map_err(|_| SpawnError::BadEdictNumber { number })
    }

    fn find_field(&self, name: &[u8]) -> Option<FieldDef> {
        let def = self.progs.ed_find_field(name)?;
        let slot = usize::try_from(def.ofs).ok()?;
        // The field's declared type decides the value conversion (EDICT-8.2); an unknown type
        // number names no conversion, so the pair is applied as a plain float slot.
        let etype = match def.def_type() {
            DefType::Known(etype) => etype,
            DefType::Unknown(_) => qc_types::Etype::Float,
        };
        Some(FieldDef { slot, etype })
    }

    fn find_function(&self, name: &[u8]) -> Option<i32> {
        // ED_FindFunction returns the function; its index is its position in the table.
        let target = self.progs.ed_find_function(name)?;
        self.progs
            .functions()
            .iter()
            .position(|f| f == target)
            .and_then(|i| i32::try_from(i).ok())
    }

    fn global(&self, slot: usize) -> Option<Slot> {
        let index = i32::try_from(slot).ok()?;
        self.host.machine.global(index).ok()
    }

    fn set_field(&mut self, edict: usize, field: usize, value: Slot) {
        self.host.set_field(edict, field, value);
    }

    fn make_string(&mut self, text: Vec<u8>) -> i32 {
        self.host.make_string(text)
    }

    fn spawnflags(&self, edict: usize) -> f32 {
        self.host.field(edict, SPAWNFLAGS_SLOT).as_f32()
    }

    fn spawn(&mut self, edict: usize, function: i32) -> Result<(), qc_vm::RunError> {
        // EDICT-8.6: set self to the entity and call the function; nothing else is set.
        self.host.set_self(edict);
        self.host.run_function(self.progs, function)
    }

    fn free(&mut self, edict: usize) {
        // EDICT-7: ED_Free at the map-start clock.
        let clock = self.host.builtins.engine().clock;
        self.host.machine.edicts_mut().free(edict, clock);
    }
}
