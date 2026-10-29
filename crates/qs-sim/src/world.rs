// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/sv_main.c, SV_SpawnServer
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_edict.c, ED_Alloc, ED_Free, ED_FindField, ED_FindFunction

//! A map's world: the progs' VM with a map's entities spawned in it, as `SV_SpawnServer` spawns
//! them (EDICT-4.1, EDICT-8, EDICT-11.4), and what each entity in use shows.
//!
//! The builtins are `qc-builtins`' mock engine for now, which keeps the state a spawn function
//! sets (the model list, `setmodel`'s `modelindex`) without the engine's physics: the server
//! frames that drop monsters to the floor and run their thinks are the rest of P3-24.

use qc_builtins::MockEngine;
use qc_edict::system::{field_by_name, global_by_name};
use qc_progs::Progs;
use qc_types::{DefType, Etype, Slot};
use qc_vm::{Machine, NoObserver, RunError, ServerState};
use qs_bsp::Bsp;

use crate::builtins::{ModelFiles, SimBuiltins, SimCall};
use crate::physics::Sound;

use crate::spawn::{self, FieldDef, LoadError, SPAWNFLAGS_SLOT, SpawnError, SpawnHost};

/// `MAP_START_TIME` (EDICT-8.1): the server's clock while the map's entities spawn.
const MAP_START_TIME: f64 = 1.0;
/// `SOLID_BSP` (EDICT-10.3): the world is solid.
const SOLID_BSP: f32 = 4.0;
/// `MOVETYPE_PUSH` (EDICT-10.2): the world moves as a pusher.
const MOVETYPE_PUSH: f32 = 7.0;
/// The client slots a single-player world has.
const MAXCLIENTS: usize = 1;

/// Why a world could not be made.
#[derive(Debug)]
pub enum WorldError {
    /// The progs cannot back an edict store.
    Progs(String),
    /// Spawning the map's entities failed (EDICT-8).
    Spawn(LoadError),
    /// A server frame failed (PHYS-1).
    Frame(RunError),
}

impl core::fmt::Display for WorldError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Progs(message) => write!(f, "the progs cannot run: {message}"),
            Self::Spawn(LoadError::Spawn(error)) => write!(f, "spawning failed: {error}"),
            Self::Spawn(LoadError::Run(error)) => {
                write!(f, "a spawn function failed: {}", error.message())
            }
            Self::Frame(error) => write!(f, "a server frame failed: {}", error.message()),
        }
    }
}

impl core::error::Error for WorldError {}

/// What an entity in use shows: its model and where and how it stands.
#[derive(Clone, Debug, PartialEq)]
pub struct Entity {
    /// The edict's number.
    pub number: usize,
    /// The model `modelindex` names in the model list, `maps/<map>.bsp`, `*n` for a brush
    /// submodel, or a file such as `progs/soldier.mdl`.
    pub model: String,
    /// `origin`.
    pub origin: [f32; 3],
    /// `angles`: pitch, yaw and roll in degrees.
    pub angles: [f32; 3],
    /// `frame`.
    pub frame: f32,
    /// `skin`.
    pub skin: f32,
}

/// A map's world: the VM with its entities spawned, linked in the world's collision.
pub struct World {
    machine: Machine,
    builtins: SimBuiltins,
}

impl World {
    /// The world of the map `map`, read from `bsp`, with the entities of its entity lump
    /// spawned through `progs`, as `SV_SpawnServer` makes it for single player: the world's
    /// fields, then the globals, then the entities (EDICT-4.1, EDICT-11.4, EDICT-8). The
    /// builtins link each entity in the world's collision (WORLD-8); `files` reads the other
    /// models the progs precaches.
    ///
    /// # Errors
    ///
    /// [`WorldError::Progs`] when the progs cannot back an edict store, and
    /// [`WorldError::Spawn`] when spawning fails.
    pub fn load(
        progs: &Progs<'_>,
        map: &str,
        bsp: &Bsp<'_>,
        files: ModelFiles,
    ) -> Result<Self, WorldError> {
        let machine =
            Machine::new(progs, MAXCLIENTS).map_err(|e| WorldError::Progs(e.to_string()))?;
        let submodels = bsp.models.len().saturating_sub(1);
        let mut engine = MockEngine::new(map, submodels, 0);
        engine.clock = MAP_START_TIME;
        engine.server_active = false;
        engine.clients_active = vec![false; MAXCLIENTS];
        let mut world = Self {
            machine,
            builtins: SimBuiltins::new(engine, map, bsp, files),
        };
        let entities = bsp.entities();
        // EDICT-4.1: the world's fields.
        let model = world.make_string(format!("maps/{map}.bsp").into_bytes());
        world.set_field(0, field_slot("model"), Slot::from_i32(model));
        world.set_field(0, field_slot("modelindex"), Slot::from_f32(1.0));
        world.set_field(0, field_slot("solid"), Slot::from_f32(SOLID_BSP));
        world.set_field(0, field_slot("movetype"), Slot::from_f32(MOVETYPE_PUSH));
        // EDICT-11.4: the globals.
        let mapname = world.make_string(map.as_bytes().to_vec());
        world.set_global("mapname", Slot::from_i32(mapname));
        world.set_global("serverflags", Slot::from_f32(0.0));
        world.set_global("time", Slot::from_f32(1.0));
        // EDICT-8: the entities, for single player at the normal skill.
        let mut spawner = Spawner {
            world: &mut world,
            progs,
        };
        spawn::load_from_file(&mut spawner, entities, false, 1).map_err(WorldError::Spawn)?;
        world.builtins.mock.engine_mut().server_active = true;
        Ok(world)
    }

    /// Runs one server frame of `frametime` seconds through `progs`, the progs the world was
    /// loaded with (PHYS-1).
    ///
    /// # Errors
    ///
    /// [`WorldError::Frame`] when a function the frame runs fails, or the engine stops.
    pub fn frame(&mut self, progs: &Progs<'_>, frametime: f64) -> Result<(), WorldError> {
        let clients = self.builtins.mock.engine().clients_active.clone();
        SimCall::frame(&mut self.machine, progs, &mut self.builtins)
            .physics(frametime, &clients)
            .map_err(WorldError::Frame)
    }

    /// The server's time in seconds, `sv.time`.
    #[must_use]
    pub fn time(&self) -> f64 {
        self.builtins.mock.engine().clock
    }

    /// The sounds the server has started since the last call, oldest first.
    pub fn take_sounds(&mut self) -> Vec<Sound> {
        core::mem::take(&mut self.builtins.sounds)
    }

    /// Every entity in use that has a model, the world first.
    #[must_use]
    pub fn entities(&self) -> Vec<Entity> {
        let edicts = self.machine.edicts();
        let float =
            |n: usize, field: &str| edicts.field(n, field_slot(field)).map_or(0.0, Slot::as_f32);
        let vector = |n: usize, field: &str| {
            let first = field_slot(field);
            [0, 1, 2].map(|i| {
                edicts
                    .field(n, first.saturating_add(i))
                    .map_or(0.0, Slot::as_f32)
            })
        };
        let precache = &self.builtins.mock.engine().model_precache;
        (0..edicts.num_edicts())
            .filter(|&n| edicts.is_free(n) == Some(false))
            .filter_map(|n| {
                let index = whole(float(n, "modelindex"))?;
                let model = precache.get(index).filter(|m| !m.is_empty())?.clone();
                Some(Entity {
                    number: n,
                    model,
                    origin: vector(n, "origin"),
                    angles: vector(n, "angles"),
                    frame: float(n, "frame"),
                    skin: float(n, "skin"),
                })
            })
            .collect()
    }

    fn make_string(&mut self, text: Vec<u8>) -> i32 {
        self.builtins.mock.engine_mut().strings.make(text)
    }

    fn set_field(&mut self, edict: usize, field: usize, value: Slot) {
        self.machine.edicts_mut().set_field(edict, field, value);
    }

    fn set_global(&mut self, name: &str, value: Slot) {
        if let Some(slot) = global_by_name(name).and_then(|v| i32::try_from(v.slot).ok())
            && self.machine.set_global(slot, value).is_err()
        {
            // A system global of a progs that loaded is always in range.
        }
    }

    /// The entity value of edict `edict`, id's `EDICT_TO_PROG` (EDICT-1.5).
    fn entity_value(&self, edict: usize) -> i32 {
        let size = self.machine.edicts().layout().edict_size();
        i32::try_from(edict)
            .ok()
            .and_then(|n| n.checked_mul(size))
            .unwrap_or(0)
    }
}

/// The slot of the system field named `name` (EDICT-3.1); 0 for an unknown name.
fn field_slot(name: &str) -> usize {
    field_by_name(name).map_or(0, |var| var.slot)
}

/// `value` as an index when it is a whole number at least 0.
fn whole(value: f32) -> Option<usize> {
    let small = (0..=u16::MAX).find(|&n| f32::from(n).to_bits() == value.to_bits())?;
    Some(usize::from(small))
}

/// Drives the EDICT-8 port against a [`World`] and its progs.
struct Spawner<'a, 'p> {
    world: &'a mut World,
    progs: &'a Progs<'p>,
}

impl SpawnHost for Spawner<'_, '_> {
    fn allocate(&mut self, first: bool) -> Result<usize, SpawnError> {
        if first {
            // EDICT-4.2: the map's first entity goes into edict 0 without clearing it.
            return Ok(0);
        }
        let clock = self.world.builtins.mock.engine().clock;
        self.world
            .machine
            .edicts_mut()
            .alloc(clock)
            .map_err(|_| SpawnError::NoFreeEdicts)
    }

    fn edict_value(&self, number: i32) -> Result<i32, SpawnError> {
        self.world
            .machine
            .edicts()
            .edict_num(number)
            .map(|entity| entity.0)
            .map_err(|_| SpawnError::BadEdictNumber { number })
    }

    fn find_field(&self, name: &[u8]) -> Option<FieldDef> {
        let def = self.progs.ed_find_field(name)?;
        let slot = usize::try_from(def.ofs).ok()?;
        let etype = match def.def_type() {
            DefType::Known(etype) => etype,
            DefType::Unknown(_) => Etype::Float,
        };
        Some(FieldDef { slot, etype })
    }

    fn find_function(&self, name: &[u8]) -> Option<i32> {
        let target = self.progs.ed_find_function(name)?;
        self.progs
            .functions()
            .iter()
            .position(|f| f == target)
            .and_then(|i| i32::try_from(i).ok())
    }

    fn global(&self, slot: usize) -> Option<Slot> {
        self.world.machine.global(i32::try_from(slot).ok()?).ok()
    }

    fn set_field(&mut self, edict: usize, field: usize, value: Slot) {
        self.world.set_field(edict, field, value);
    }

    fn make_string(&mut self, text: Vec<u8>) -> i32 {
        self.world.make_string(text)
    }

    fn spawnflags(&self, edict: usize) -> f32 {
        self.world
            .machine
            .edicts()
            .field(edict, SPAWNFLAGS_SLOT)
            .map_or(0.0, Slot::as_f32)
    }

    fn spawn(&mut self, edict: usize, function: i32) -> Result<(), RunError> {
        // EDICT-8.6: set self to the entity and call the function.
        let value = self.world.entity_value(edict);
        self.world.set_global("self", Slot::from_i32(value));
        self.world.machine.execute_program(
            self.progs,
            ServerState::Loading,
            &mut self.world.builtins,
            &mut NoObserver,
            function,
        )
    }

    fn free(&mut self, edict: usize) {
        let clock = self.world.builtins.mock.engine().clock;
        // WORLD-8.3: ED_Free unlinks.
        self.world.builtins.collision.unlink(edict);
        self.world.machine.edicts_mut().free(edict, clock);
    }
}

#[cfg(test)]
mod tests;
