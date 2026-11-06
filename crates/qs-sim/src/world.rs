// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/sv_main.c, SV_SpawnServer, SV_SaveSpawnparms
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/host_cmd.c, Host_Changelevel_f
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/cmd.c, Cbuf_Execute
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_edict.c, ED_Alloc, ED_Free, ED_FindField, ED_FindFunction

//! A map's world: the progs' VM with a map's entities spawned in it, as `SV_SpawnServer` spawns
//! them (EDICT-4.1, EDICT-8, EDICT-11.4), and what each entity in use shows.
//!
//! The builtins are `qc-builtins`' mock engine for now, which keeps the state a spawn function
//! sets (the model list, `setmodel`'s `modelindex`) without the engine's physics: the server
//! frames that drop monsters to the floor and run their thinks are the rest of P3-24.

use qc_builtins::MockEngine;
use qc_builtins::mock::{Cvars, Rng};
use qc_edict::EdictNumber;
use qc_edict::system::{field_by_name, global_by_name};
use qc_progs::Progs;
use qc_types::{DefType, Etype, Slot};
use qc_vm::convert::double_to_float;
use qc_vm::{Machine, NoObserver, RunError, ServerState};
use qs_bsp::Bsp;
use qs_vfs::Registration;

use crate::builtins::{ModelFiles, SimBuiltins, SimCall};
use crate::client::UserCmd;
use crate::debug::Debugger;
use crate::host_commands::SignonCommand;
use crate::messages::ClientMessages;
#[cfg(test)]
use crate::messages::ServerMessages;
use crate::physics::Sound;
use crate::rerelease::BuiltinTable;
use crate::settings::SERVER_SETTINGS;
use qc_builtins::localization::Localization;

use crate::spawn::{
    self, FieldDef, LoadError, Parser, SPAWNFLAGS_SLOT, SpawnError, SpawnHost, Token,
};

/// `MAP_START_TIME` (EDICT-8.1): the server's clock while the map's entities spawn.
const MAP_START_TIME: f64 = 1.0;
/// `SOLID_BSP` (EDICT-10.3): the world is solid.
const SOLID_BSP: f32 = 4.0;
/// `MOVETYPE_PUSH` (EDICT-10.2): the world moves as a pusher.
const MOVETYPE_PUSH: f32 = 7.0;
/// The client slots a single-player world has.
const MAXCLIENTS: usize = 1;
/// The single player's edict.
const CLIENT: usize = CLIENT_EDICT;

/// The player's edict, the one client's slot (EDICT-5.1).
pub const CLIENT_EDICT: usize = 1;
/// The random source's first state (MOVE-6.1).
const RANDOM_SEED: u32 = 1;
/// `EDICT_SETTLE_FRAMETIME`: the two frames the server runs before play (EDICT-8.9).
const SETTLE_FRAMETIME: f64 = 0.1;

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

/// A map's world: the VM with its entities spawned, linked in the world's collision. A clone
/// is a snapshot the debugger replays a frame from (WORLD-9).
#[derive(Clone)]
pub struct World {
    machine: Machine,
    pub(crate) builtins: SimBuiltins,
    /// The client's command, once it has entered the game (PHYS-9.6, PHYS-10).
    player: Option<UserCmd>,
    /// The string commands the client sent since the last frame, which it reads first
    /// (PHYS-11.1).
    pub(crate) commands: Vec<Vec<u8>>,
    /// Whether the client connects in the next frame (PHYS-10.7).
    connecting: bool,
    /// The lines the console printed since they were taken (PHYS-13).
    pub(crate) console_out: Vec<u8>,
    /// A host command the console asked for, waiting for the host (PHYS-13.6).
    pub(crate) pending_host: Option<HostCommand>,
    /// The console's aliases, newest first (PHYS-13.2).
    pub(crate) aliases: Vec<(String, String)>,
    /// The search path the host gave, for `path` (PHYS-13.7).
    pub(crate) search_path: Vec<String>,
    /// The spawn parms the level the player left kept, used in place of `SetNewParms`'.
    carried: Option<SpawnParms>,
    /// The client's spawn parms, `host_client->spawn_parms`, which a saved game keeps.
    spawn_parms: [Slot; 16],
    /// Whether a saved game was read into this world, `sv.loadgame` (PHYS-10.5).
    loaded: bool,
    /// The map's name, `sv.name`.
    map: String,
}

/// What the host gives a world besides its map and progs.
#[derive(Clone, Debug, Default)]
pub struct LoadOptions {
    /// The rerelease's localized strings, from `localization/loc_english.txt`; with none the
    /// builtins send their text as id's engine does (RRE-1.6).
    pub localization: Localization,
    /// The builtin table, as [`crate::rerelease::patch_builtins`] chose it for the progs
    /// (RRE-4.2).
    pub builtins: BuiltinTable,
    /// What the level the player left carries into this one, `None` for a new game
    /// (PHYS-10.4).
    pub carried: Option<SpawnParms>,
    /// Whether the install is the full game, which sets the `registered` setting the progs
    /// read (FS-4.5).
    pub registration: Registration,
    /// What the last level's host had, kept for this one: `None` for a new game, where the
    /// random source starts at 1 and the settings at their defaults (MOVE-6.3, PHYS-12.3).
    pub host: Option<HostState>,
    /// The search path's elements as the engine names them, most important first, for the
    /// console's `path` (PHYS-13.7).
    pub search_path: Vec<String>,
    /// The state the random source starts at in a new game, a scenario's `seed` (SCN-1.3);
    /// `None` for MOVE-6.1's 1. A level carrying the last one's host state keeps its source.
    pub seed: Option<u32>,
}

/// What lives in id's engine for the whole process, not one level: the C library's random
/// state and the settings (MOVE-6.3, PHYS-12.3). [`World::host_state`] takes it from one level
/// for the next.
#[derive(Clone, Debug, PartialEq)]
pub struct HostState {
    rng: Rng,
    cvars: Cvars,
    /// The console's text not yet run and its aliases (PHYS-13.6).
    buffer: Vec<u8>,
    aliases: Vec<(String, String)>,
}

/// The stages of a client's entry, `CL_SignonReply`'s answers and the server's commands for
/// them (PHYS-10.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signon {
    /// `SV_ConnectClient`: the client's slot taken and its spawn parms chosen.
    Connect,
    /// `prespawn`, `Host_PreSpawn_f`: the map's signon data sent.
    Prespawn,
    /// `spawn`, `Host_Spawn_f`: the player's edict made.
    Spawn,
    /// `begin`, `Host_Begin_f`: the player moves from the next frame.
    Begin,
}

/// A command the progs leave for the host with `localcmd` or `changelevel`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostCommand {
    /// `changelevel <map>`: the next level (PHYS-10.4).
    Changelevel(String),
    /// `restart`: this level again, as single player's `respawn` asks (PHYS-10.6).
    Restart,
    /// `map <map>`: a new game on the map, carrying nothing (PHYS-13.6).
    Map(String),
    /// `save <name>`: the game saved as `<name>.sav` (PHYS-10.5, PHYS-13.6).
    Save(String),
    /// `load <name>`: the game in `<name>.sav` loaded (PHYS-10.5, PHYS-13.6).
    Load(String),
    /// `quit`: the host stops.
    Quit,
}

/// What `SV_SaveSpawnparms` keeps across a level change: `serverflags`, "for cross level
/// information (sigils)", and the client's `parm1` to `parm16` after `SetChangeParms`
/// (PHYS-10.4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpawnParms {
    serverflags: Slot,
    parms: [Slot; 16],
}

impl SpawnParms {
    /// `parm1` to `parm16` as floats, as the progs reads them.
    #[must_use]
    pub fn parms(&self) -> [f32; 16] {
        self.parms.map(Slot::as_f32)
    }

    /// `serverflags` as a float.
    #[must_use]
    pub fn serverflags(&self) -> f32 {
        self.serverflags.as_f32()
    }
}

impl World {
    /// The world of the map `map`, read from `bsp`, with the entities of its entity lump
    /// spawned through `progs`, as `SV_SpawnServer` makes it for single player: the world's
    /// fields, then the globals, then the entities (EDICT-4.1, EDICT-11.4, EDICT-8). The
    /// builtins link each entity in the world's collision (WORLD-8); `files` reads the other
    /// models the progs precaches, and `options` holds what the host loaded before the map.
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
        options: LoadOptions,
    ) -> Result<Self, WorldError> {
        let machine =
            Machine::new(progs, MAXCLIENTS).map_err(|e| WorldError::Progs(e.to_string()))?;
        let submodels = bsp.models.len().saturating_sub(1);
        // MOVE-6.1: the random source starts at state 1, or at a scenario's seed (SCN-1.3).
        let seed = options.seed.unwrap_or(RANDOM_SEED);
        let mut engine = MockEngine::new(map, submodels, seed);
        let aliases = options
            .host
            .as_ref()
            .map(|h| h.aliases.clone())
            .unwrap_or_default();
        match options.host {
            // MOVE-6.3, PHYS-12.3: the last level's random state and settings go on.
            Some(host) => {
                engine.rng = host.rng;
                engine.cvars = host.cvars;
                engine.command_buffer = host.buffer;
            }
            // PHYS-12.1: the server's and the host's settings, with id's defaults.
            None => {
                for (name, value) in SERVER_SETTINGS {
                    engine.cvars.create(name, value);
                }
            }
        }
        engine.clock = MAP_START_TIME;
        engine.server_active = false;
        engine.clients_active = vec![false; MAXCLIENTS];
        // RRE-1.6: QuakeSpasm loads the file when the host starts, before any map spawns.
        engine.localization = options.localization;
        // FS-4.5: `registered`, 0 until `COM_CheckRegistered` finds the full game.
        engine
            .cvars
            .create("registered", options.registration.cvar());
        let mut world = Self {
            machine,
            builtins: SimBuiltins::new(
                engine,
                map,
                bsp,
                files,
                progs.header().crc,
                options.builtins,
            ),
            player: None,
            commands: Vec::new(),
            connecting: false,
            console_out: Vec::new(),
            pending_host: None,
            aliases,
            search_path: options.search_path,
            carried: options.carried,
            spawn_parms: [Slot::ZERO; 16],
            loaded: false,
            map: map.to_owned(),
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
        // PHYS-10.4: `pr_global_struct->serverflags = svs.serverflags`, 0 for a new game.
        let serverflags = options
            .carried
            .map_or(Slot::from_f32(0.0), |c| c.serverflags);
        world.set_global("serverflags", serverflags);
        world.set_global("time", Slot::from_f32(1.0));
        // EDICT-8: the entities, for single player at the normal skill.
        let mut spawner = Spawner {
            world: &mut world,
            progs,
            state: ServerState::Loading,
        };
        spawn::load_from_file(&mut spawner, entities, false, 1).map_err(WorldError::Spawn)?;
        world.builtins.mock.engine_mut().server_active = true;
        Ok(world)
    }

    /// The rest of `SV_SpawnServer`: the two settle frames, run without the `frametime`
    /// global (EDICT-8.9), then the entity baselines (NET-2.1). A host calls it once after
    /// [`World::load`], before the player enters; the baselines are made at
    /// [`World::connect`] if it did not.
    ///
    /// # Errors
    ///
    /// [`WorldError::Frame`] when a settle frame fails, or a model is not precached.
    pub fn settle(&mut self, progs: &Progs<'_>) -> Result<(), WorldError> {
        let clients = self.builtins.mock.engine().clients_active.clone();
        let mut call = SimCall::frame(&mut self.machine, progs, &mut self.builtins);
        for _ in 0..2 {
            call.physics(SETTLE_FRAMETIME, &clients)
                .map_err(WorldError::Frame)?;
        }
        call.create_baselines().map_err(WorldError::Frame)
    }

    /// The view angles the server told the client to take since the last call,
    /// `svc_setangle` as the client reads it (PHYS-10.2, NET-4.3).
    pub fn take_set_angle(&mut self) -> Option<[f32; 3]> {
        self.builtins.messages.set_angle.take()
    }

    /// Whether the game is standard Quake, not `-rogue` or `-hipnotic`, which changes how the
    /// weapon is sent (NET-4.4); standard by default.
    pub fn set_standard_quake(&mut self, standard: bool) {
        self.builtins.messages.standard_quake = standard;
    }

    /// The messages sent to the client since the last call (NET-1.3): each packet a reliable
    /// message or a datagram, in sending order.
    pub fn take_messages(&mut self) -> ClientMessages {
        ClientMessages {
            packets: core::mem::take(&mut self.builtins.messages.outbox),
        }
    }

    /// Runs one server frame of `frametime` seconds through `progs`, the progs the world was
    /// loaded with (PHYS-1).
    ///
    /// # Errors
    ///
    /// [`WorldError::Frame`] when a function the frame runs fails, or the engine stops.
    pub fn frame(&mut self, progs: &Progs<'_>, frametime: f64) -> Result<(), WorldError> {
        // MOVE-6.2: `_Host_Frame`'s `rand ();`, "keep the random time dependent".
        let before = self.builtins.mock.engine().rng;
        let _discarded = self.builtins.mock.engine_mut().rng.next_rand();
        if let Some(debugger) = &mut self.builtins.debugger {
            debugger.begin_frame();
        }
        // PHYS-13.1: `_Host_Frame`'s `Cbuf_Execute`, before the server frame; a host command
        // it meets waits for the host, and this frame does not run (PHYS-13.6). id runs the
        // command and then the server frame in one host frame with one `rand ()`, so the draw
        // is given back for the frame the host runs after it has acted.
        self.execute_console(progs);
        if self.pending_host.is_some() {
            self.builtins.mock.engine_mut().rng = before;
            return Ok(());
        }
        {
            let mut call = SimCall::frame(&mut self.machine, progs, &mut self.builtins);
            // Host_ServerFrame (NET-1.2): frametime, the datagram cleared, then the clients'
            // moves, then the physics, then the messages.
            call.set_global("frametime", Slot::from_f32(double_to_float(frametime)));
            call.sim.messages.datagram.clear();
            // The mock engine counts the datagram's bytes beside the sim's (NET-5.1): it
            // empties with it.
            call.sim.mock.engine_mut().buffers.unreliable = 0;
        }
        // PHYS-10.7: `SV_CheckForNewClients`, then `SV_RunClients` reading the client's string
        // commands, before its move (PHYS-11.1), whether or not it has spawned yet.
        if core::mem::take(&mut self.connecting) {
            self.signon(progs, Signon::Connect)?;
        }
        if self.builtins.messages.client.active {
            for text in core::mem::take(&mut self.commands) {
                let spawned = self.player.is_some();
                let stage = SimCall::frame(&mut self.machine, progs, &mut self.builtins)
                    .client_command(CLIENT, &text, spawned)
                    .map_err(WorldError::Frame)?;
                match stage {
                    Some(SignonCommand::Prespawn) => self.signon(progs, Signon::Prespawn)?,
                    Some(SignonCommand::Spawn) => self.signon(progs, Signon::Spawn)?,
                    Some(SignonCommand::Begin) => self.signon(progs, Signon::Begin)?,
                    None => {}
                }
            }
        }
        let clients = self.builtins.mock.engine().clients_active.clone();
        let player = self.player;
        // PHYS-11.8: paused, the client's move is read but not thought, and no physics runs.
        let paused = self.builtins.paused;
        let mut call = SimCall::frame(&mut self.machine, progs, &mut self.builtins);
        if let Some(cmd) = player {
            let cmd = call.read_move(CLIENT, &cmd);
            if !paused {
                call.client_think(CLIENT, &cmd, frametime)
                    .map_err(WorldError::Frame)?;
            }
        }
        if !paused {
            call.physics(frametime, &clients)
                .map_err(WorldError::Frame)?;
        }
        call.send_client_messages().map_err(WorldError::Frame)?;
        // The reliable datagram went out with the messages (NET-1.4).
        call.sim.mock.engine_mut().buffers.reliable = 0;
        Ok(())
    }

    /// The random state and the settings, for the next level's [`LoadOptions::host`]
    /// (MOVE-6.3, PHYS-12.3).
    #[must_use]
    pub fn host_state(&self) -> HostState {
        let engine = self.builtins.mock.engine();
        HostState {
            rng: engine.rng,
            cvars: engine.cvars.clone(),
            buffer: engine.command_buffer.clone(),
            aliases: self.aliases.clone(),
        }
    }

    /// The debugger the frames run with, `None` to run without one (WORLD-9.1).
    pub fn set_debugger(&mut self, debugger: Option<Debugger>) {
        self.builtins.debugger = debugger;
    }

    /// The debugger, to set breakpoints and rules and to read where a run stopped.
    pub fn debugger_mut(&mut self) -> Option<&mut Debugger> {
        self.builtins.debugger.as_mut()
    }

    /// The debugger, made over `lines` (WORLD-9.4) when the world has none yet.
    pub fn debugger_or_new(&mut self, lines: &[i32]) -> &mut Debugger {
        self.builtins
            .debugger
            .get_or_insert_with(|| Debugger::new(lines.to_vec()))
    }

    /// The debugger, to read where a run stopped.
    #[must_use]
    pub const fn debugger(&self) -> Option<&Debugger> {
        self.builtins.debugger.as_ref()
    }

    /// The machine, for reading the call stack and the locals while stopped (WORLD-9.5).
    #[must_use]
    pub const fn machine(&self) -> &Machine {
        &self.machine
    }

    /// The client connects in the next frame, at its `SV_CheckForNewClients` (PHYS-10.7);
    /// it then sends `prespawn`, `spawn` and `begin` as string commands.
    pub fn connect_next_frame(&mut self) {
        self.connecting = true;
    }

    /// The player enters the game, as `SV_ConnectClient` and `Host_Spawn_f` bring a client in
    /// (PHYS-10): every stage of [`Signon`] at once, as a host that does not count the frames
    /// between them does.
    ///
    /// # Errors
    ///
    /// [`WorldError::Frame`] when a function fails.
    pub fn connect(&mut self, progs: &Progs<'_>) -> Result<(), WorldError> {
        for stage in [
            Signon::Connect,
            Signon::Prespawn,
            Signon::Spawn,
            Signon::Begin,
        ] {
            self.signon(progs, stage)?;
        }
        Ok(())
    }

    /// One stage of the player's entry (PHYS-10.7): a local client's take a frame each, the
    /// server running each before that frame's moves and physics.
    ///
    /// # Errors
    ///
    /// [`WorldError::Frame`] when a function fails.
    pub fn signon(&mut self, progs: &Progs<'_>, stage: Signon) -> Result<(), WorldError> {
        match stage {
            Signon::Connect => self.signon_connect(progs),
            Signon::Prespawn => {
                let mut call = SimCall::frame(&mut self.machine, progs, &mut self.builtins);
                call.prespawn();
                call.flush_signon();
                Ok(())
            }
            Signon::Spawn => self.signon_spawn(progs),
            Signon::Begin => {
                let mut call = SimCall::frame(&mut self.machine, progs, &mut self.builtins);
                call.begin();
                self.player = Some(UserCmd::default());
                Ok(())
            }
        }
    }

    /// `SV_ConnectClient` (PHYS-10.7): the client is active, the serverinfo is sent, and its
    /// spawn parms are `SetNewParms`', or what it carries from the last level (PHYS-10.4).
    fn signon_connect(&mut self, progs: &Progs<'_>) -> Result<(), WorldError> {
        if let Some(active) = self.builtins.mock.engine_mut().clients_active.first_mut() {
            *active = true;
        }
        self.builtins.messages.client.active = true;
        let carried = self.carried;
        let loaded = self.loaded;
        let mut call = SimCall::frame(&mut self.machine, progs, &mut self.builtins);
        if call.sim.messages.baselines.is_empty() {
            call.create_baselines().map_err(WorldError::Frame)?;
        }
        // NET-2.2: the serverinfo, sent at the end of the frame the stage ran in.
        call.send_serverinfo(CLIENT);
        call.flush_signon();
        // PHYS-10.5: a saved game's world keeps the parms it saved.
        if loaded {
            return Ok(());
        }
        let parms: [Slot; 16] = match carried {
            Some(carried) => carried.parms,
            None => {
                let new_parms = call.global("SetNewParms").as_i32();
                call.run(new_parms).map_err(WorldError::Frame)?;
                core::array::from_fn(|n| call.global(&format!("parm{}", n.saturating_add(1))))
            }
        };
        self.spawn_parms = parms;
        Ok(())
    }

    /// `Host_Spawn_f` (PHYS-10.1): the client's edict cleared and named, its parms set, then
    /// `ClientConnect` and `PutClientInServer`, and the spawn message (NET-2.4).
    fn signon_spawn(&mut self, progs: &Progs<'_>) -> Result<(), WorldError> {
        let name = self.make_string(self.builtins.client_name.clone());
        let parms = self.spawn_parms;
        let loaded = self.loaded;
        let mut call = SimCall::frame(&mut self.machine, progs, &mut self.builtins);
        // PHYS-10.5: a saved game's world is "fully inited allready": no spawn functions.
        if loaded {
            return call.spawn_message(CLIENT).map_err(WorldError::Frame);
        }
        let fields = call.context.machine_view().edicts().layout().entityfields();
        for slot in 0..fields {
            // A slot past the edict's fields is not written.
            let _written = call.context.machine().edicts_mut().set_field(
                EdictNumber::new(CLIENT),
                slot,
                Slot::from_i32(0),
            );
        }
        call.set_float(CLIENT, "colormap", 1.0);
        call.set_float(CLIENT, "team", 1.0);
        call.set_entity_raw(CLIENT, "netname", name);
        for (n, parm) in (1..=16).zip(parms) {
            call.set_global(&format!("parm{n}"), parm);
        }
        call.set_time(call.clock());
        let value = call.value_of(CLIENT);
        call.set_global("self", Slot::from_i32(value));
        for entry in ["ClientConnect", "PutClientInServer"] {
            let function = call.global(entry).as_i32();
            call.run(function).map_err(WorldError::Frame)?;
        }
        call.spawn_message(CLIENT).map_err(WorldError::Frame)
    }

    /// The command for the host the console met in the last frame's `Cbuf_Execute`, from the
    /// player or the progs' `localcmd` (PHYS-13.6): `changelevel <map>`, which
    /// `Host_Changelevel_f` takes only with exactly one argument (PHYS-10.4), `restart`
    /// (PHYS-10.6), `map`, `save`, `load` or `quit`. It is taken once.
    pub fn take_host_command(&mut self) -> Option<HostCommand> {
        self.pending_host.take()
    }

    /// The map's name, `sv.name`, which `restart` spawns again (PHYS-10.6).
    #[must_use]
    pub fn map(&self) -> &str {
        &self.map
    }

    /// The parms the player entered this level with, `None` in a new game, which `restart`
    /// keeps (PHYS-10.6).
    #[must_use]
    pub const fn carried(&self) -> Option<SpawnParms> {
        self.carried
    }

    /// `SV_SaveSpawnparms`: `serverflags`, then, with `self` the client, `SetChangeParms`
    /// and the client's `parm1` to `parm16`, for the next level's [`LoadOptions::carried`];
    /// a client not yet active keeps the parms it has (PHYS-10.4).
    ///
    /// # Errors
    ///
    /// [`WorldError::Frame`] when `SetChangeParms` fails.
    pub fn save_spawn_parms(&mut self, progs: &Progs<'_>) -> Result<SpawnParms, WorldError> {
        let mut call = SimCall::frame(&mut self.machine, progs, &mut self.builtins);
        let serverflags = call.global("serverflags");
        // `if (!host_client->active) continue;`
        if self.player.is_none() {
            return Ok(SpawnParms {
                serverflags,
                parms: self.spawn_parms,
            });
        }
        let value = call.value_of(CLIENT);
        call.set_global("self", Slot::from_i32(value));
        let change = call.global("SetChangeParms").as_i32();
        call.run(change).map_err(WorldError::Frame)?;
        let parms = core::array::from_fn(|n| call.global(&format!("parm{}", n.saturating_add(1))));
        Ok(SpawnParms { serverflags, parms })
    }

    /// A string command from the client, such as `god`, read at the start of the next frame
    /// (PHYS-11).
    pub fn client_command(&mut self, text: &str) {
        self.commands.push(text.as_bytes().to_vec());
    }

    /// What the player asks for from the next frame on (PHYS-9.6).
    pub fn set_command(&mut self, cmd: UserCmd) {
        if self.player.is_some() {
            self.player = Some(cmd);
        }
    }

    /// The player's eye: its `origin + view_ofs`, and its `v_angle`; `None` before it has
    /// entered the game.
    #[must_use]
    pub fn player_view(&self) -> Option<([f32; 3], [f32; 3])> {
        self.player?;
        let edicts = self.machine.edicts();
        let vector = |name: &str| {
            let first = field_slot(name);
            [0, 1, 2].map(|i| {
                edicts
                    .field(EdictNumber::new(CLIENT), first.saturating_add(i))
                    .map_or(0.0, Slot::as_f32)
            })
        };
        let [x, y, z] = vector("origin");
        let [a, b, c] = vector("view_ofs");
        Some(([x + a, y + b, z + c], vector("v_angle")))
    }

    /// The wall time the frames' runs of QuakeC have taken since the world loaded, the
    /// QuakeC time per frame of the benchmarks (P3-29, ENG-17); the simulation never reads it.
    #[must_use]
    pub fn qc_time(&self) -> std::time::Duration {
        self.builtins.qc_time
    }

    /// The server's time in seconds, `sv.time`.
    #[must_use]
    pub fn time(&self) -> f64 {
        self.builtins.mock.engine().clock
    }

    /// The string the progs last set for light style `style` with `lightstyle`, if any
    /// (BUILTIN-12.6), which the renderer animates (REND-5).
    #[must_use]
    pub fn light_style(&self, style: usize) -> Option<&str> {
        self.builtins
            .mock
            .engine()
            .lightstyles
            .get(style)
            .and_then(Option::as_deref)
    }

    /// The sounds the server has started since the last call, oldest first.
    pub fn take_sounds(&mut self) -> Vec<Sound> {
        core::mem::take(&mut self.builtins.sounds)
    }

    /// Every entity in use that has a model, the world first.
    #[must_use]
    pub fn entities(&self) -> Vec<Entity> {
        let edicts = self.machine.edicts();
        let float = |n: usize, field: &str| {
            edicts
                .field(EdictNumber::new(n), field_slot(field))
                .map_or(0.0, Slot::as_f32)
        };
        let vector = |n: usize, field: &str| {
            let first = field_slot(field);
            [0, 1, 2].map(|i| {
                edicts
                    .field(EdictNumber::new(n), first.saturating_add(i))
                    .map_or(0.0, Slot::as_f32)
            })
        };
        let precache = &self.builtins.mock.engine().model_precache;
        (0..edicts.num_edicts())
            .filter(|&n| edicts.is_free(EdictNumber::new(n)) == Some(false))
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

    #[cfg(test)]
    pub(crate) fn messages(&self) -> &ServerMessages {
        &self.builtins.messages
    }

    #[cfg(test)]
    pub(crate) fn engine(&self) -> &MockEngine {
        self.builtins.mock.engine()
    }

    fn make_string(&mut self, text: Vec<u8>) -> i32 {
        self.builtins.mock.engine_mut().strings.make(text)
    }

    fn set_field(&mut self, edict: usize, field: usize, value: Slot) {
        // A slot past the edict's fields is not written.
        let _written = self
            .machine
            .edicts_mut()
            .set_field(EdictNumber::new(edict), field, value);
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
    /// `sv.state` while the spawn functions run: loading for the map's entities, where the
    /// precache builtins work, active for an entity spawned in play, where they stop the run
    /// (`PF_precache_model`: "Precache can only be done in spawn functions").
    state: ServerState,
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
            .map(EdictNumber::index)
            .map_err(|_| SpawnError::NoFreeEdicts)
    }

    fn edict_value(&self, number: i32) -> Result<i32, SpawnError> {
        self.world
            .machine
            .edicts()
            .edict_num(number)
            .map(|entity| entity.bits())
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
            .field(EdictNumber::new(edict), SPAWNFLAGS_SLOT)
            .map_or(0.0, Slot::as_f32)
    }

    fn spawn(&mut self, edict: usize, function: i32) -> Result<(), RunError> {
        // EDICT-8.6: set self to the entity and call the function.
        let value = self.world.entity_value(edict);
        self.world.set_global("self", Slot::from_i32(value));
        self.world.machine.execute_program(
            self.progs,
            self.state,
            &mut self.world.builtins,
            &mut NoObserver,
            function,
        )
    }

    fn free(&mut self, edict: usize) {
        let clock = self.world.builtins.mock.engine().clock;
        // WORLD-8.3: ED_Free unlinks.
        self.world.builtins.collision.unlink(edict);
        self.world
            .machine
            .edicts_mut()
            .free(EdictNumber::new(edict), clock);
    }
}

/// `Cbuf_Execute`'s commands in `text`: split at a line end, or at a `;` outside quotes
/// ("don't break if inside a quoted string") (PHYS-10.4).
fn commands(text: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut line = Vec::new();
    let mut quoted = false;
    for &c in text {
        if c == b'"' {
            quoted = !quoted;
        }
        if c == b'\n' || (c == b';' && !quoted) {
            out.push(core::mem::take(&mut line));
            quoted = false;
        } else {
            line.push(c);
        }
    }
    out.push(line);
    out
}

/// The commands of console text with their words: `Cbuf_Execute`'s commands, each split
/// as `Cmd_TokenizeString` splits it with `COM_Parse`; a command with no words, which
/// `Cmd_ExecuteString` skips, is left out.
#[must_use]
pub fn command_words(text: &[u8]) -> Vec<Vec<Vec<u8>>> {
    commands(text)
        .into_iter()
        .map(|command| {
            let mut parser = Parser::new(&command);
            let mut words = Vec::new();
            while let Token::Word(word) = parser.next_token() {
                words.push(word);
            }
            words
        })
        .filter(|words| !words.is_empty())
        .collect()
}

mod changes;
mod edit;
mod evaluate;
mod inspect;
mod reload;
mod save;
mod state;
#[cfg(test)]
pub(crate) mod tests;

pub use changes::EdictChange;
pub use edit::{EditError, LineHit};
pub use evaluate::EvalError;
pub use inspect::{NamedValue, QcValue};
pub use reload::{Change, ChangeKind, Migration, Reason};
pub use save::{SAVEGAME_VERSION, SaveError, SaveGame};
pub use state::{RawState, State, read_state};
