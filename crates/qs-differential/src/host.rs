// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/sv_main.c, SV_SpawnServer, SV_ConnectClient
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/host_cmd.c, Host_Spawn_f
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/host.c, Host_ServerFrame, Host_ShutdownServer, SV_DropClient
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/sv_phys.c, SV_Physics, SV_Physics_Client, SV_Physics_Pusher, SV_RunThink

//! QuakeC Studio's differential host loop (DIFF-3 to DIFF-5, DIFF-8).
//!
//! The host stands in for id's server (DIFF-1.5): it sets up the map (DIFF-3), lets the
//! scenario's clients enter (DIFF-4), runs the server frames (DIFF-5) and, after a runtime or
//! host error, shuts the server down (DIFF-8). It drives `qc-vm` with `qc-edict` and the
//! `qc-builtins` mock engine, and every call into QuakeC writes trace records (TRACE-4) into
//! one [`TraceBuilder`]; the map's entities are spawned by the EDICT-8 port
//! ([`qs_sim::spawn`]). The loop around those is this module's, following the oracle's
//! `scenario.c` and id's `SV_SpawnServer` and `SV_Physics`, so the two runners write the same
//! trace (DIFF-1.1).
//!
//! Implements `docs/spec/differential.md` DIFF-3, DIFF-4, DIFF-5 and DIFF-8, with the think
//! timing of EDICT-10 (DIFF-5.5, DIFF-5.6) against the frame vectors of `edicts.md` and
//! `differential.md`, and writes the trace of `docs/spec/trace.md` (TRACE-1 to TRACE-4) around
//! the VM's own records.

use qc_builtins::mock::ModelBounds as MockModelBounds;
use qc_builtins::{MockBuiltins, MockEngine};
use qc_edict::EdictNumber;
use qc_edict::system::{SysField, SysGlobal};
use qc_progs::Progs;
use qc_types::Slot;
use qc_vm::{Machine, RunError, ServerState};

use crate::scenario::Scenario;
use qs_sim::spawn::{self, LoadError};

mod frames;
mod spawner;
mod trace;

use spawner::Spawner;
pub use trace::TraceBuilder;
use trace::{StringBuiltins, StringObserver, StringQueue};

/// `EDICT_MAP_START_TIME` (EDICT-8.1, DIFF-3.2): the clock when a map starts.
pub const MAP_START_TIME: f64 = 1.0;
/// `EDICT_SETTLE_FRAMETIME` (EDICT-8.9, DIFF-3.6): the length of each settle frame.
pub const SETTLE_FRAMETIME: f64 = 0.1;
/// `EDICT_SETTLE_FRAMES` (EDICT-8.9): the settle frames run before the clients enter.
pub const SETTLE_FRAMES: u32 = 2;
/// `DIFF_CLIENT_NAME` (DIFF-4.1): the `netname` a client that enters is given.
pub const CLIENT_NAME: &[u8] = b"player";
/// `SOLID_BSP` (EDICT-4.1): the world's `solid`.
const SOLID_BSP: f32 = 4.0;

/// `MOVETYPE_NONE` (EDICT-10.2).
const MOVETYPE_NONE: i32 = 0;
/// `MOVETYPE_WALK` (EDICT-10.2): a client movetype that thinks.
const MOVETYPE_WALK: i32 = 3;
/// `MOVETYPE_STEP` (EDICT-10.2).
const MOVETYPE_STEP: i32 = 4;
/// `MOVETYPE_FLY` (EDICT-10.2).
const MOVETYPE_FLY: i32 = 5;
/// `MOVETYPE_TOSS` (EDICT-10.2).
const MOVETYPE_TOSS: i32 = 6;
/// `MOVETYPE_PUSH` (EDICT-4.1, EDICT-10.4, DIFF-5.5): the world's movetype.
const MOVETYPE_PUSH: i32 = 7;
/// `MOVETYPE_NOCLIP` (EDICT-10.2).
const MOVETYPE_NOCLIP: i32 = 8;
/// `MOVETYPE_FLYMISSILE` (EDICT-10.2).
const MOVETYPE_FLYMISSILE: i32 = 9;
/// `MOVETYPE_BOUNCE` (EDICT-10.2).
const MOVETYPE_BOUNCE: i32 = 10;
/// `EF_MUZZLEFLASH` (EDICT-10.7, DIFF-5.1): cleared after each frame's updates.
const EF_MUZZLEFLASH: i32 = 2;
/// `FL_ITEM` (EDICT-12.2, DIFF-5.7): a link box widened by 15 on x and y, not 1.
const FL_ITEM: i32 = 256;
/// `NUM_SPAWN_PARMS` (EDICT-2.1): `parm1` to `parm16`.
const NUM_SPAWN_PARMS: usize = 16;

/// How a scenario ended (DIFF-8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ended {
    /// The scenario ran to the end of its frames with no error (DIFF-3.7).
    Normally,
    /// An error ended it (DIFF-8): after a runtime or host error with the server active the
    /// server was shut down (DIFF-8.3); after a fatal error, or one while the map loaded,
    /// nothing more ran (DIFF-8.6). The error record is already in the trace (TRACE-4.6).
    Error,
}

/// Why the host could not run a scenario before any trace was written (DIFF-3.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunRefused {
    /// The progs could not be read, or cannot back an edict store (DIFF-3.1).
    Progs(String),
    /// `maxclients` is out of range for the edict store (EDICT-5.1).
    Edict(String),
    /// A trace header could not be built (TRACE-2.3); unreachable for a loaded progs.
    Trace(String),
}

impl core::fmt::Display for RunRefused {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Progs(m) | Self::Edict(m) | Self::Trace(m) => f.write_str(m),
        }
    }
}

impl core::error::Error for RunRefused {}

/// What a differential run produced: the trace and how the scenario ended.
#[derive(Clone, Debug)]
pub struct RunOutput {
    /// The whole trace (TRACE-1.1), ready to encode with `qc_trace::encode`.
    pub trace: qc_trace::Trace,
    /// How the scenario ended (DIFF-8).
    pub ended: Ended,
}

/// Runs one scenario end to end in QuakeC Studio's runner and returns its trace (DIFF-1.1).
///
/// `progs_bytes` is the progs.dat the scenario names, read into memory, and `entities` the
/// map's entity lump as EDICT-8 reads it (DIFF-2.5); no trailing NUL is needed, since the
/// EDICT-8 parser stops at the end of the slice or a NUL. The run loads the progs (DIFF-3.1),
/// writes the trace header (DIFF-3.2), spawns the map (DIFF-3.4, DIFF-3.5), runs the settle
/// frames (DIFF-3.6), lets the clients enter (DIFF-4), runs the scenario's frames (DIFF-5) and,
/// on an error, shuts the server down (DIFF-8).
///
/// # Errors
///
/// [`RunRefused`] when the progs is refused or cannot back an edict store (DIFF-3.1); no trace
/// is written.
pub fn run_scenario(
    scenario: &Scenario,
    progs_bytes: &[u8],
    entities: &[u8],
) -> Result<RunOutput, RunRefused> {
    // DIFF-3.1: load the progs with the header CRC checked (PROGS-3).
    let progs = Progs::read(progs_bytes).map_err(|e| RunRefused::Progs(e.to_string()))?;
    let mut host = Host::new(scenario, &progs)?;
    let ended = host.run(&progs, entities);
    Ok(RunOutput {
        trace: host.trace.finish(),
        ended,
    })
}

impl<'s> Host<'s> {
    /// The server for `scenario` with `progs` loaded, before the map (DIFF-3.1, DIFF-3.2): the
    /// VM, the trace header and the mock engine with the scenario's models and settings.
    ///
    /// # Errors
    ///
    /// [`RunRefused`] when the progs cannot back an edict store or a trace header.
    pub(crate) fn new(scenario: &'s Scenario, progs: &Progs<'_>) -> Result<Self, RunRefused> {
        let maxclients = usize::try_from(scenario.maxclients).unwrap_or(0);
        let machine =
            Machine::new(progs, maxclients).map_err(|e| RunRefused::Edict(e.to_string()))?;
        // DIFF-3.2: the trace header, written after loading and before the first run (TRACE-1.3).
        let trace =
            TraceBuilder::new(&machine, progs).map_err(|e| RunRefused::Trace(e.to_string()))?;

        let submodels = usize::try_from(scenario.submodels).unwrap_or(0);
        let mut engine = MockEngine::new(&scenario.map, submodels, scenario.seed);
        for model in &scenario.models {
            if let Ok(name) = core::str::from_utf8(&model.name) {
                engine.model_bounds.push((
                    name.to_owned(),
                    MockModelBounds {
                        mins: model.mins,
                        maxs: model.maxs,
                    },
                ));
            }
        }
        for cvar in &scenario.cvars {
            if let (Ok(name), Ok(text)) = (
                core::str::from_utf8(&cvar.name),
                core::str::from_utf8(&cvar.text),
            ) {
                engine.cvars.set(name, text);
            }
        }

        Ok(Host {
            machine,
            builtins: MockBuiltins::with_engine(engine),
            trace,
            scenario,
            entered: vec![false; maxclients],
        })
    }
}

/// The whole running server: the VM, the mock engine, the trace and the scenario (DIFF-1.2).
pub(crate) struct Host<'s> {
    pub(crate) machine: Machine,
    pub(crate) builtins: MockBuiltins,
    pub(crate) trace: TraceBuilder,
    pub(crate) scenario: &'s Scenario,
    /// Whether each client slot's player has entered (DIFF-4.2); index `n - 1` is slot `n`.
    entered: Vec<bool>,
}

impl Host<'_> {
    /// Runs the scenario: the map, the clients, the frames and the shutdown (DIFF-3 to DIFF-8).
    fn run(&mut self, progs: &Progs<'_>, entities: &[u8]) -> Ended {
        // DIFF-8.1, DIFF-8.3: an error while the map loads ends the scenario with no shutdown,
        // since the server is not yet active. A run's error already has its record (TRACE-4.6);
        // an error the host raises while spawning, outside any run, gets one with function 0
        // and statement -1 (DIFF-8.1).
        match self.start_map(progs, entities) {
            Ok(()) => {}
            Err(LoadError::Spawn(error)) => {
                self.trace
                    .host_error(&self.machine, 0, -1, &error.to_string());
                return Ended::Error;
            }
            Err(LoadError::Run(_)) => return Ended::Error,
        }
        // DIFF-4: the clients enter.
        for slot in 0..self.scenario.clients {
            if let Err(error) = self.client_enters(progs, slot) {
                return self.shut_down(progs, &error);
            }
        }
        // DIFF-5.1: the scenario's frames.
        for frame in 1..=self.scenario.frames {
            if let Err(error) = self.server_frame(progs, self.scenario.frametime, frame) {
                return self.shut_down(progs, &error);
            }
        }
        Ended::Normally
    }

    /// DIFF-3: set up the map, spawn its entities and run the settle frames.
    pub(crate) fn start_map(
        &mut self,
        progs: &Progs<'_>,
        entities: &[u8],
    ) -> Result<(), LoadError> {
        // DIFF-3.2: deathmatch becomes 0 when coop is non-zero; the skill is clamped to 0..3.
        let coop = self.setting("coop");
        let skill = clamp_skill(self.setting("skill"));

        // DIFF-3.2: the engine state. Machine::new gave 600 zero edicts with
        // num_edicts = maxclients + 1 (EDICT-1.8); the clock is 1.0 and the server is loading.
        self.builtins.engine_mut().server_active = false;
        self.builtins.engine_mut().clients_active = vec![false; self.entered.len()];
        self.builtins.engine_mut().clock = MAP_START_TIME;

        // DIFF-3.4, EDICT-4.1: the world's fields, before the globals.
        let world_model = format!("maps/{}.bsp", self.scenario.map);
        let model_string = self.make_string(world_model.into_bytes());
        self.set_field(0, SysField::Model.slot(), Slot::from_i32(model_string));
        self.set_field(0, SysField::Modelindex.slot(), Slot::from_f32(1.0));
        self.set_field(0, SysField::Solid.slot(), Slot::from_f32(SOLID_BSP));
        self.set_field(0, SysField::Movetype.slot(), movetype_slot(MOVETYPE_PUSH));

        // DIFF-3.4, EDICT-11.4: the globals.
        if coop != 0.0 {
            self.set_global(SysGlobal::Coop, Slot::from_f32(coop));
        } else {
            self.set_global(
                SysGlobal::Deathmatch,
                Slot::from_f32(self.setting("deathmatch")),
            );
        }
        let mapname = self.make_string(self.scenario.map.clone().into_bytes());
        self.set_global(SysGlobal::Mapname, Slot::from_i32(mapname));
        self.set_global(SysGlobal::Serverflags, Slot::from_f32(0.0));
        // EDICT-8.1: the time global is set to 1 (f32), the clock at map start.
        self.set_global(SysGlobal::Time, Slot::from_f32(f32_of(MAP_START_TIME)));

        // DIFF-3.5: spawn the map's entities (EDICT-8), with the deathmatch and skill values
        // for the inhibition test (EDICT-8.4).
        let deathmatch = self.setting("deathmatch") != 0.0;
        let mut spawner = Spawner::new(self, progs);
        spawn::load_from_file(&mut spawner, entities, deathmatch, skill)?;

        // DIFF-3.6: the server becomes active and runs the two settle frames (DIFF-5.3),
        // without setting frametime, emptying a buffer or clearing muzzle flashes.
        self.builtins.engine_mut().server_active = true;
        for _ in 0..SETTLE_FRAMES {
            self.physics(progs, SETTLE_FRAMETIME)
                .map_err(LoadError::Run)?;
        }
        Ok(())
    }

    /// DIFF-4.1: a client slot's player enters, id's `SV_ConnectClient` then `Host_Spawn_f`.
    pub(crate) fn client_enters(&mut self, progs: &Progs<'_>, slot: u32) -> Result<(), RunError> {
        let slot_index = usize::try_from(slot).unwrap_or(usize::MAX);
        let edict = slot_index.saturating_add(1);

        // SV_ConnectClient: run SetNewParms with nothing set (EDICT-9.1), then save parm1..16.
        self.run_named(progs, SysGlobal::Setnewparms)?;
        let mut spawn_parms = [Slot::ZERO; NUM_SPAWN_PARMS];
        for (i, cell) in spawn_parms.iter_mut().enumerate() {
            *cell = self.global_slot(parm_slot(i));
        }

        // Host_Spawn_f: zero the slot's edict without changing its free flag.
        if let Some(fields) = self
            .machine
            .edicts_mut()
            .fields_mut(EdictNumber::new(edict))
        {
            fields.fill(Slot::ZERO);
        }
        // colormap = edict number, team = 1, netname = engine string "player" (DIFF-4.1).
        let colormap = u8::try_from(edict).map_or(0.0, f32::from);
        self.set_field(edict, SysField::Colormap.slot(), Slot::from_f32(colormap));
        self.set_field(edict, SysField::Team.slot(), Slot::from_f32(1.0));
        let netname = self.make_string(CLIENT_NAME.to_vec());
        self.set_field(edict, SysField::Netname.slot(), Slot::from_i32(netname));

        // Copy the spawn parms back to parm1..16 (DIFF-4.1).
        for (i, value) in spawn_parms.iter().enumerate() {
            self.set_global_slot(parm_slot(i), *value);
        }

        // time = (f32)clock, self = the edict, then ClientConnect and PutClientInServer.
        let clock = self.builtins.engine().clock;
        self.set_global(SysGlobal::Time, Slot::from_f32(f32_of(clock)));
        self.set_self(edict);
        self.run_named(progs, SysGlobal::Clientconnect)?;
        self.run_named(progs, SysGlobal::Putclientinserver)?;

        // DIFF-4.2: the slot is active and spawned from now on.
        if let Some(cell) = self.entered.get_mut(slot_index) {
            *cell = true;
        }
        if let Some(cell) = self
            .builtins
            .engine_mut()
            .clients_active
            .get_mut(slot_index)
        {
            *cell = true;
        }
        Ok(())
    }

    /// DIFF-8.3: after a runtime or host error, shut the server down, running each entered
    /// client's `ClientDisconnect`, then end the scenario; after a fatal error, end it with no
    /// shutdown (DIFF-8.6).
    ///
    /// Each `ClientDisconnect` run starts at the depth the error left (DIFF-8.4): 0 after a
    /// runtime error, the depth at the error after a host error (`qc_vm` keeps it,
    /// [`qc_vm::ErrorKind::resets_depth`]).
    fn shut_down(&mut self, progs: &Progs<'_>, error: &RunError) -> Ended {
        // The error record is already written by the failed run's observer (TRACE-4.6), or by
        // the host for an error outside a run (DIFF-8.1).
        if error.kind.is_fatal() {
            // DIFF-8.6: Sys_Error calls Host_Shutdown, which runs no QuakeC, and exits.
            return Ended::Error;
        }
        // The server is active (from the settle frames on), so Host_ShutdownServer runs.
        for slot in 0..self.entered.len() {
            if self.entered.get(slot).copied() != Some(true) {
                continue;
            }
            let edict = slot.saturating_add(1);
            // ClientDisconnect saves and restores self (EDICT-9.2).
            let saved = self.global_slot(SysGlobal::SelfEntity.ofs());
            self.set_self(edict);
            if self.run_named(progs, SysGlobal::Clientdisconnect).is_err() {
                // DIFF-8.5: an error during the shutdown ends the scenario at once.
                return Ended::Error;
            }
            self.set_global_slot(SysGlobal::SelfEntity.ofs(), saved);
        }
        Ended::Error
    }

    // --- helpers shared with the frame and spawner code ---

    /// The value of the setting named `name`, `Q_atof` of its text (DIFF-2.4, BUILTIN-11.7);
    /// an unknown setting is 0.
    pub(crate) fn setting(&self, name: &str) -> f32 {
        self.builtins
            .engine()
            .cvars
            .text(name)
            .map_or(0.0, |text| spawn::c_atof(text.as_bytes()))
    }

    /// Runs the QuakeC function named by the system global `name`, id's `qs_run` (EDICT-9.1).
    ///
    /// The function index is read from the global; a 0 or out-of-range function fails with
    /// `PR_ExecuteProgram: NULL function` (EDICT-9.4, VM-2.1), which the VM reports.
    pub(crate) fn run_named(
        &mut self,
        progs: &Progs<'_>,
        global: SysGlobal,
    ) -> Result<(), RunError> {
        let function = self.global_slot(global.ofs()).as_i32();
        self.run_function(progs, function)
    }

    /// Runs QuakeC function `function`, id's `PR_ExecuteProgram` (EDICT-9.1), through the VM
    /// with the mock builtins and the trace observer.
    ///
    /// The VM is given the server state the mock keys its loading-only builtins on
    /// (`precache_sound`, `precache_model`, BUILTIN-10.1, and the entity-0 write of VM-14.7):
    /// `Loading` while the map is still spawning its entities (DIFF-3.2, DIFF-3.5) and `Active`
    /// once the server is active (DIFF-3.6). id's `sv.state` is `ss_loading` through
    /// `SV_SpawnServer` until the map has spawned, then `ss_active`, so a spawn function may
    /// precache and a later run may not.
    pub(crate) fn run_function(
        &mut self,
        progs: &Progs<'_>,
        function: i32,
    ) -> Result<(), RunError> {
        let server = if self.builtins.engine().server_active {
            ServerState::Active
        } else {
            ServerState::Loading
        };
        // DIFF-3.9: an engine string a builtin sets is recorded before the CALL's step record.
        let queue = StringQueue::default();
        let mut builtins = StringBuiltins {
            builtins: &mut self.builtins,
            queue: &queue,
        };
        let mut observer = StringObserver {
            trace: &mut self.trace,
            queue: &queue,
        };
        self.machine
            .execute_program(progs, server, &mut builtins, &mut observer, function)
    }

    /// Makes an engine string holding `text`, records it in the trace (TRACE-4.10) and returns
    /// its value (EDICT-13.1).
    pub(crate) fn make_string(&mut self, text: Vec<u8>) -> i32 {
        let value = self.builtins.engine_mut().strings.make(text.clone());
        self.trace.string_set(value, text);
        value
    }

    /// Writes field slot `field` of edict `edict` (EDICT-1.6).
    pub(crate) fn set_field(&mut self, edict: usize, field: usize, value: Slot) {
        // A slot past the edict's fields is not written.
        let _written = self
            .machine
            .edicts_mut()
            .set_field(EdictNumber::new(edict), field, value);
    }

    /// Reads field slot `field` of edict `edict`, or 0 when it does not exist.
    pub(crate) fn field(&self, edict: usize, field: usize) -> Slot {
        self.machine
            .edicts()
            .field(EdictNumber::new(edict), field)
            .unwrap_or(Slot::ZERO)
    }

    /// Writes the system global `global` (EDICT-2.1).
    pub(crate) fn set_global(&mut self, global: SysGlobal, value: Slot) {
        self.set_global_slot(global.ofs(), value);
    }

    /// Writes global slot `slot`, ignoring an out-of-range slot (it cannot happen for a system
    /// global of a progs that loaded).
    pub(crate) fn set_global_slot(&mut self, slot: i32, value: Slot) {
        if self.machine.set_global(slot, value).is_err() {
            // A system global of a loaded progs is always in range; ignore otherwise.
        }
    }

    /// Reads global slot `slot`, or 0 when out of range.
    pub(crate) fn global_slot(&self, slot: i32) -> Slot {
        self.machine.global(slot).unwrap_or(Slot::ZERO)
    }

    /// Sets the `self` global to the entity value of edict `edict` (EDICT-9.1).
    pub(crate) fn set_self(&mut self, edict: usize) {
        let value = self.entity_value(edict);
        self.set_global(SysGlobal::SelfEntity, Slot::from_i32(value));
    }

    /// Sets the `other` global to the entity value of edict `edict` (EDICT-9.1).
    pub(crate) fn set_other(&mut self, edict: usize) {
        let value = self.entity_value(edict);
        self.set_global(SysGlobal::Other, Slot::from_i32(value));
    }

    /// The entity value of edict `edict`, id's `EDICT_TO_PROG` (EDICT-1.5).
    pub(crate) fn entity_value(&self, edict: usize) -> i32 {
        let size = self.machine.edicts().layout().edict_size();
        i32::try_from(edict)
            .ok()
            .and_then(|n| n.checked_mul(size))
            .unwrap_or(0)
    }
}

/// Clamps the skill as DIFF-3.2 does: `(int)(skill + 0.5)` clamped to 0 to 3.
fn clamp_skill(skill: f32) -> i32 {
    ftoi(skill + 0.5).clamp(0, 3)
}

/// `(int)` as BUILTIN-2.2 fixes it: truncation toward zero, `i32::MIN` outside the range.
fn ftoi(value: f32) -> i32 {
    let d = f64::from(value);
    if d.is_nan() || d >= 2_147_483_648.0 || d <= -2_147_483_649.0 {
        return i32::MIN;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "spec BUILTIN-2.2: truncation toward zero, range checked above"
    )]
    let truncated = d as i32;
    truncated
}

/// A `movetype` field value as its `f32` bits (the field is a float, EDICT-3.1).
fn movetype_slot(movetype: i32) -> Slot {
    #[expect(
        clippy::cast_precision_loss,
        reason = "spec EDICT-4.1: movetype is a small whole number stored as an f32"
    )]
    let value = movetype as f32;
    Slot::from_f32(value)
}

/// The `f32` nearest a clock or frame length, as C's assignment of a `double` to a `float`
/// rounds it ("(f32)x" in `differential.md`, DIFF-3.4, DIFF-5.1).
pub(crate) fn f32_of(value: f64) -> f32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "spec DIFF-5.1: the f32 nearest the double, as C rounds it"
    )]
    let rounded = value as f32;
    rounded
}

/// The global slot of `parm<n+1>` (EDICT-2.1): `parm1` is slot 43, so `parm<n+1>` is 43 + n.
fn parm_slot(n: usize) -> i32 {
    let base = SysGlobal::Parm1.ofs();
    base.checked_add(i32::try_from(n).unwrap_or(0))
        .unwrap_or(base)
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;
