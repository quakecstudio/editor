// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2002-2009 John Fitzgibbons and others
// Copyright (C) 2010-2014 QuakeSpasm developers
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_cmds.c, PF_setorigin, PF_setsize, PF_setmodel, PF_traceline, PF_droptofloor, PF_walkmove, PF_checkbottom, PF_pointcontents
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_edict.c, ED_Free
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/pr_cmds.c, PF_finalefinished, PF_CheckPlayerEXFlags, PF_walkpathtogoal, PF_Fixme

//! The server's builtins: `qc-builtins`' for the engine state it keeps, and the world builtins
//! on this crate's collision (WORLD-8) and movement (MOVE).

use qc_builtins::math::vector_normalize;
use std::rc::Rc;

use qc_builtins::mock::{BUILTIN_CHECKCLIENT_INTERVAL, BUILTIN_FL_NOTARGET, ModelBounds};
use qc_builtins::{MockBuiltins, MockEngine};
use qc_edict::EdictNumber;
use qc_edict::system::{field_by_name, global_by_name};
use qc_progs::Progs;
use qc_types::{EntityRef, PROGS_OFS_RETURN, Slot, ofs_parm};
use qc_vm::convert::{double_to_float, float_to_int, int_to_float};
use qc_vm::{Builtins, Context, ErrorKind, Machine, RunError, ServerState};
use qs_bsp::{Bsp, BspLoader};
use qs_mdl::Sprite;

use crate::collision::{
    Body, Collision, CollisionError, Host, MoveKind, Solid, Trace, link_edict, sv_move,
};
use crate::debug::{Debugger, Watch};
use crate::messages::{SVC_CENTERPRINT, SVC_PRINT, ServerMessages};
use crate::physics::{Sound, dot};
use crate::rerelease::{BuiltinTable, RRE_NUM_BUILTINS};
use crate::settings::Setting;

/// `setorigin`.
const SETORIGIN: u32 = 2;
/// `setmodel`.
const SETMODEL: u32 = 3;
/// `setsize`.
const SETSIZE: u32 = 4;
/// `remove`.
const REMOVE: u32 = 15;
/// `traceline`.
const TRACELINE: u32 = 16;
/// `checkclient`, builtin 17 (WORLD-8.7).
const CHECKCLIENT: u32 = 17;
/// `aim`, builtin 44 (WORLD-8.8).
const AIM: u32 = 44;
/// `DAMAGE_AIM` (`WinQuake/server.h`): what `aim` aims at (WORLD-8.8).
const DAMAGE_AIM: f32 = 2.0;
/// `precache_model`.
const PRECACHE_MODEL: u32 = 20;
/// `walkmove`.
const WALKMOVE: u32 = 32;
/// `droptofloor`.
const DROPTOFLOOR: u32 = 34;
/// `checkbottom`.
const CHECKBOTTOM: u32 = 40;
/// `pointcontents`.
const POINTCONTENTS: u32 = 41;
/// `movetogoal`.
const MOVETOGOAL: u32 = 67;
/// `makestatic`.
const MAKESTATIC: u32 = 69;
/// `sound` (BUILTIN-12.1).
const SOUND: u32 = 8;
/// `stuffcmd` (BUILTIN-11.4).
const STUFFCMD: u32 = 21;
/// `bprint` (BUILTIN-11.1).
const BPRINT: u32 = 23;
/// `sprint` (BUILTIN-11.2).
const SPRINT: u32 = 24;
/// `lightstyle` (BUILTIN-12.6).
const LIGHTSTYLE: u32 = 35;
/// `particle` (BUILTIN-12.5).
const PARTICLE: u32 = 48;
/// `WriteByte`, the first of the `Write*` builtins (BUILTIN-13).
const WRITEBYTE: u32 = 52;
/// `WriteEntity`, the last of them.
const WRITEENTITY: u32 = 59;
/// `centerprint` (BUILTIN-11.2).
const CENTERPRINT: u32 = 73;
/// `ambientsound` (BUILTIN-12.4).
const AMBIENTSOUND: u32 = 74;
/// `precache_model2`.
const PRECACHE_MODEL2: u32 = 75;
/// `end[2] -= 256` in `PF_droptofloor` (WORLD-8.5).
const DROP_DEPTH: f32 = 256.0;
/// `MOVETYPE_PUSH` (WORLD-1.4).
const MOVETYPE_PUSH: f32 = 7.0;
/// `FL_ONGROUND`.
pub(crate) const FL_ONGROUND: i32 = 512;

/// Reads a model file the progs precaches, such as `maps/b_bh25.bsp`.
pub type ModelFiles = Rc<dyn Fn(&str) -> Option<Vec<u8>>>;

/// The builtins a world runs its progs with.
#[derive(Clone)]
pub struct SimBuiltins {
    /// The engine state builtins read and write, and every builtin but the world's.
    pub(crate) mock: MockBuiltins,
    /// The world's collision.
    pub(crate) collision: Collision,
    /// The sounds the server started, oldest first (PHYS-4.4, PHYS-5.1).
    pub(crate) sounds: Vec<Sound>,
    /// The messages to the client (NET-1 to NET-5).
    pub(crate) messages: ServerMessages,
    /// id's table, or the rerelease's with its builtins 79 to 92 (RRE-4.2).
    pub(crate) table: BuiltinTable,
    /// The debugger, the frames' runs' observer when a host debugs (WORLD-9).
    pub(crate) debugger: Option<Debugger>,
    /// The statements each function has run since `profile` last printed, id's
    /// `dfunction_t.profile` (PHYS-13.7).
    pub(crate) profile: Vec<u64>,
    /// The wall time the frames' runs of QuakeC have taken, for the QuakeC time benchmark
    /// (P3-29, ENG-17): measured, never read by the simulation.
    pub(crate) qc_time: std::time::Duration,
    /// Whether the server is paused, `sv.paused` (PHYS-11.8).
    pub(crate) paused: bool,
    /// The client's name, `host_client->name`, which `spawn` gives its `netname`.
    pub(crate) client_name: Vec<u8>,
    /// The PVS of the client `checkclient` chose last, id's `checkpvs`.
    checkpvs: Vec<u8>,
    pub(crate) files: ModelFiles,
}

impl SimBuiltins {
    /// The builtins of the map `map`, named `name`, with its brush models' bounds for
    /// `setmodel` (WORLD-8.2), reading other models with `files`.
    pub(crate) fn new(
        engine: MockEngine,
        name: &str,
        map: &Bsp<'_>,
        files: ModelFiles,
        crc: i32,
        table: BuiltinTable,
    ) -> Self {
        let mut mock = MockBuiltins::with_engine(engine);
        let bounds = &mut mock.engine_mut().model_bounds;
        for (n, model) in map.models.iter().enumerate() {
            let key = if n == 0 {
                format!("maps/{name}.bsp")
            } else {
                format!("*{n}")
            };
            bounds.push((
                key,
                ModelBounds {
                    mins: model.mins,
                    maxs: model.maxs,
                },
            ));
        }
        Self {
            mock,
            collision: Collision::new(map),
            sounds: Vec::new(),
            messages: ServerMessages::new(map, crc),
            table,
            debugger: None,
            profile: Vec::new(),
            qc_time: std::time::Duration::ZERO,
            paused: false,
            client_name: b"player".to_vec(),
            checkpvs: Vec::new(),
            files,
        }
    }

    /// The model just precached as model index `index`, `name`: a BSP is loaded for its hulls
    /// and bounds, a sprite for its bounds; an alias model has the mock's (WORLD-8.2).
    fn loaded(&mut self, index: usize, name: &str) {
        let bounds = if name.ends_with(".bsp") {
            let Some(bytes) = (self.files)(name) else {
                return;
            };
            let Ok(bsp) = Bsp::read(&bytes, BspLoader::default()) else {
                return;
            };
            self.collision.add_brush(index, &bsp);
            bsp.models.first().map(|m| (m.mins, m.maxs))
        } else if name.ends_with(".spr") {
            (self.files)(name)
                .and_then(|bytes| Sprite::read(&bytes).ok().map(|s| s.bounds()))
                .map(|(mins, maxs)| (mins.map(f32_of), maxs.map(f32_of)))
        } else {
            None
        };
        if let Some((mins, maxs)) = bounds {
            self.mock
                .engine_mut()
                .model_bounds
                .push((name.to_owned(), ModelBounds { mins, maxs }));
        }
    }
}

/// An `i32` bound as the float the model stores.
#[expect(
    clippy::cast_precision_loss,
    reason = "spec WORLD-8.2: a sprite's bounds are small ints id stores as floats"
)]
const fn f32_of(value: i32) -> f32 {
    value as f32
}

/// `finaleFinished`, the first of the rerelease's builtins (RRE-4.2).
const FINALE_FINISHED: u32 = 79;
/// `localsound` (RRE-2.1).
const LOCALSOUND: u32 = 80;
/// `CheckPlayerEXFlags` (RRE-4.2).
const CHECK_PLAYER_EX_FLAGS: u32 = 90;
/// `walkpathtogoal` (RRE-4.2).
const WALK_PATH_TO_GOAL: u32 = 91;

impl SimBuiltins {
    /// QuakeSpasm's builtins 79 to 92 (RRE-4.2): `finaleFinished`, `CheckPlayerEXFlags` and
    /// `walkpathtogoal` return 0, `localsound` sends its sound (RRE-2.1), and the rest are
    /// `PF_Fixme`'s "unimplemented builtin".
    fn rerelease_call(
        &mut self,
        number: u32,
        context: &mut Context<'_, '_>,
    ) -> Result<(), RunError> {
        let mut call = SimCall::new(context, self);
        match number {
            FINALE_FINISHED | CHECK_PLAYER_EX_FLAGS | WALK_PATH_TO_GOAL => call.set_return(0.0),
            LOCALSOUND => call.local_sound(),
            _ => Err(call.context.run_error(ErrorKind::Builtin {
                message: "unimplemented builtin".to_owned(),
            })),
        }
    }
}

impl Builtins for SimBuiltins {
    fn limit(&self) -> Option<u32> {
        match self.table {
            BuiltinTable::Id => self.mock.limit(),
            BuiltinTable::Rerelease => Some(RRE_NUM_BUILTINS),
        }
    }

    fn call(&mut self, number: u32, context: &mut Context<'_, '_>) -> Result<(), RunError> {
        if self.table == BuiltinTable::Rerelease && number >= FINALE_FINISHED {
            return self.rerelease_call(number, context);
        }
        match number {
            // WORLD-8.1: the mock sets the fields; the world links.
            SETORIGIN | SETMODEL | SETSIZE => {
                self.mock.call(number, context)?;
                let ent = parm_edict(&Vm::Builtin(context), 0)?;
                SimCall::new(context, self).link(ent, false)
            }
            // WORLD-8.3: ED_Free unlinks.
            REMOVE => {
                self.mock.call(number, context)?;
                let ent = parm_edict(&Vm::Builtin(context), 0)?;
                self.collision.unlink(ent);
                Ok(())
            }
            MAKESTATIC => {
                // NET-5.1: the static's bytes are read before the mock frees the edict.
                let bytes = SimCall::new(context, self).static_bytes()?;
                let before = self.mock.engine().buffers.signon;
                self.mock.call(number, context)?;
                if self.mock.engine().buffers.signon > before {
                    self.messages.signon.write_bytes(&bytes);
                }
                let ent = parm_edict(&Vm::Builtin(context), 0)?;
                self.collision.unlink(ent);
                Ok(())
            }
            PRECACHE_MODEL | PRECACHE_MODEL2 => {
                let before = self.mock.engine().model_precache.len();
                self.mock.call(number, context)?;
                // A name not precached before is the new last entry.
                if let Some(name) = self.mock.engine().model_precache.get(before).cloned() {
                    self.loaded(before, &name);
                }
                Ok(())
            }
            // NET-5.1: the bytes of the builtins that write to the client, beside the
            // mock's counts; a buffer that did not grow had the write dropped or refused.
            SOUND => {
                let before = self.mock.engine().buffers.unreliable;
                self.mock.call(number, context)?;
                if self.mock.engine().buffers.unreliable > before {
                    SimCall::new(context, self).mirror_sound();
                }
                Ok(())
            }
            AMBIENTSOUND => {
                let before = self.mock.engine().buffers.signon;
                self.mock.call(number, context)?;
                if self.mock.engine().buffers.signon > before {
                    SimCall::new(context, self).mirror_ambientsound();
                }
                Ok(())
            }
            PARTICLE => {
                let before = self.mock.engine().buffers.unreliable;
                self.mock.call(number, context)?;
                if self.mock.engine().buffers.unreliable > before {
                    SimCall::new(context, self).mirror_particle();
                }
                Ok(())
            }
            LIGHTSTYLE => {
                self.mock.call(number, context)?;
                SimCall::new(context, self).mirror_lightstyle();
                Ok(())
            }
            BPRINT => {
                self.mock.call(number, context)?;
                SimCall::new(context, self).mirror_bprint();
                Ok(())
            }
            SPRINT => {
                self.mock.call(number, context)?;
                SimCall::new(context, self).mirror_client_print(SVC_PRINT);
                Ok(())
            }
            CENTERPRINT => {
                self.mock.call(number, context)?;
                SimCall::new(context, self).mirror_client_print(SVC_CENTERPRINT);
                Ok(())
            }
            STUFFCMD => {
                self.mock.call(number, context)?;
                SimCall::new(context, self).mirror_stuffcmd();
                Ok(())
            }
            WRITEBYTE..=WRITEENTITY => {
                let before = self.mock.engine().buffers;
                self.mock.call(number, context)?;
                // A MSG_ONE write is not counted by the mock: mirror it on success.
                let to = float_to_int(parm_float(&Vm::Builtin(context), 0));
                if to == 1 || self.mock.engine().buffers != before {
                    SimCall::new(context, self).mirror_write(number);
                }
                Ok(())
            }
            TRACELINE => SimCall::new(context, self).traceline(),
            CHECKCLIENT => SimCall::new(context, self).checkclient(),
            AIM => SimCall::new(context, self).aim(),
            DROPTOFLOOR => SimCall::new(context, self).droptofloor(),
            WALKMOVE => SimCall::new(context, self).walkmove(),
            MOVETOGOAL => SimCall::new(context, self).move_to_goal(),
            CHECKBOTTOM => {
                let ent = parm_edict(&Vm::Builtin(context), 0)?;
                let mut call = SimCall::new(context, self);
                let bottom = call.bottom(ent)?;
                call.set_return(if bottom { 1.0 } else { 0.0 })
            }
            POINTCONTENTS => {
                let p = parm_vector(&Vm::Builtin(context), 0);
                let mut call = SimCall::new(context, self);
                let contents = call
                    .sim
                    .collision
                    .point_contents(p)
                    .map_err(|e| call.error(e))?;
                call.set_return(f32::from(i16::try_from(contents).unwrap_or(i16::MIN)))
            }
            _ => self.mock.call(number, context),
        }
    }

    fn engine_string(&self, value: i32) -> Option<&[u8]> {
        self.mock.engine_string(value)
    }
}

/// The VM a call runs on: a builtin's context, whose runs nest in the caller's, or the
/// machine itself, at the top of a server frame.
pub(crate) enum Vm<'c, 'a, 'p> {
    /// Inside a builtin.
    Builtin(&'c mut Context<'a, 'p>),
    /// In the server frame, between runs.
    Frame {
        machine: &'c mut Machine,
        progs: &'c Progs<'p>,
    },
}

impl Vm<'_, '_, '_> {
    pub(crate) fn machine_view(&self) -> &Machine {
        match self {
            Self::Builtin(context) => context.machine_view(),
            Self::Frame { machine, .. } => machine,
        }
    }

    pub(crate) fn machine(&mut self) -> &mut Machine {
        match self {
            Self::Builtin(context) => context.machine(),
            Self::Frame { machine, .. } => machine,
        }
    }

    pub(crate) fn progs(&self) -> &Progs<'_> {
        match self {
            Self::Builtin(context) => context.progs(),
            Self::Frame { progs, .. } => progs,
        }
    }

    /// `PR_ExecuteProgram (function)`: nested in a builtin, or a run of its own in the frame.
    pub(crate) fn execute_program(
        &mut self,
        sim: &mut SimBuiltins,
        function: i32,
    ) -> Result<(), RunError> {
        match self {
            Self::Builtin(context) => context.execute_program(sim, function),
            Self::Frame { machine, progs } => {
                // WORLD-9.1, PHYS-13.7: the debugger, when there is one, and the profile counts
                // watch the run; they are lent so the builtins and they are not borrowed twice.
                let mut debugger = sim.debugger.take();
                let mut profile = core::mem::take(&mut sim.profile);
                let mut watch = Watch {
                    debugger: debugger.as_mut(),
                    profile: &mut profile,
                };
                let start = std::time::Instant::now();
                let outcome =
                    machine.execute_program(progs, ServerState::Active, sim, &mut watch, function);
                sim.qc_time = sim.qc_time.saturating_add(start.elapsed());
                sim.debugger = debugger;
                sim.profile = profile;
                outcome
            }
        }
    }

    /// An error the engine raises, with the current stack inside a builtin.
    pub(crate) fn run_error(&self, kind: ErrorKind) -> RunError {
        match self {
            Self::Builtin(context) => context.run_error(kind),
            Self::Frame { .. } => RunError {
                kind,
                function: 0,
                statement: -1,
                trace: Vec::new(),
            },
        }
    }
}

/// One call into the server: the VM and the server's builtins, which together answer the
/// world's queries (`Host`).
pub(crate) struct SimCall<'c, 'a, 'p> {
    pub(crate) context: Vm<'c, 'a, 'p>,
    pub(crate) sim: &'c mut SimBuiltins,
}

impl<'c, 'a, 'p> SimCall<'c, 'a, 'p> {
    pub(crate) fn new(context: &'c mut Context<'a, 'p>, sim: &'c mut SimBuiltins) -> Self {
        Self {
            context: Vm::Builtin(context),
            sim,
        }
    }

    /// A call at the top of a server frame.
    pub(crate) fn frame(
        machine: &'c mut Machine,
        progs: &'c Progs<'p>,
        sim: &'c mut SimBuiltins,
    ) -> Self {
        Self {
            context: Vm::Frame { machine, progs },
            sim,
        }
    }

    /// A world error as the run's error: id's `Sys_Error`, or the error a touch raised.
    pub(crate) fn error(&self, error: CollisionError) -> RunError {
        match error {
            CollisionError::Touch(run) => *run,
            other => self.context.run_error(ErrorKind::Fatal {
                message: other.to_string(),
            }),
        }
    }

    /// `SV_LinkEdict (ent, touch)`.
    pub(crate) fn link(&mut self, ent: usize, touch: bool) -> Result<(), RunError> {
        link_edict(self, ent, touch).map_err(|e| self.error(e))
    }

    /// `SV_Move`, ignoring `pass`.
    pub(crate) fn trace(
        &self,
        line: ([f32; 3], [f32; 3], [f32; 3], [f32; 3]),
        kind: MoveKind,
        pass: usize,
    ) -> Result<Trace, RunError> {
        sv_move(self, line, kind, Some(pass)).map_err(|e| self.error(e))
    }

    // ---- fields and globals ----

    pub(crate) fn float(&self, n: usize, name: &str) -> f32 {
        field(&self.context, n, name).map_or(0.0, Slot::as_f32)
    }

    pub(crate) fn vector(&self, n: usize, name: &str) -> [f32; 3] {
        let first = field_slot(name);
        let edicts = self.context.machine_view().edicts();
        [0, 1, 2].map(|i| {
            edicts
                .field(EdictNumber::new(n), first.saturating_add(i))
                .map_or(0.0, Slot::as_f32)
        })
    }

    /// A field's value as an int: a function, a string or an entity value.
    pub(crate) fn entity_raw(&self, n: usize, name: &str) -> i32 {
        field(&self.context, n, name).map_or(0, Slot::as_i32)
    }

    /// Field `name` of the progs, not of the system set, when the progs has one:
    /// `GetEdictFieldValue` (PHYS-6.4).
    pub(crate) fn progs_field(&self, n: usize, name: &str) -> Option<Slot> {
        let def = self.context.progs().ed_find_field(name.as_bytes())?;
        let slot = usize::try_from(def.ofs).ok()?;
        self.context
            .machine_view()
            .edicts()
            .field(EdictNumber::new(n), slot)
    }

    /// The edict an entity field names, id's `PROG_TO_EDICT`; the world for any value that
    /// is not an edict.
    pub(crate) fn entity(&self, n: usize, name: &str) -> usize {
        let value = field(&self.context, n, name).map_or(0, Slot::as_i32);
        self.edict_of(value)
    }

    pub(crate) fn edict_of(&self, value: i32) -> usize {
        let layout = self.context.machine_view().edicts().layout();
        layout
            .edict_number(EntityRef::from_bits(value))
            .map_or(0, EdictNumber::index)
    }

    pub(crate) fn value_of(&self, n: usize) -> i32 {
        let size = self.context.machine_view().edicts().layout().edict_size();
        i32::try_from(n)
            .ok()
            .and_then(|n| n.checked_mul(size))
            .unwrap_or(0)
    }

    pub(crate) fn set_float(&mut self, n: usize, name: &str, value: f32) {
        let slot = field_slot(name);
        // A slot past the edict's fields is not written.
        let _written = self.context.machine().edicts_mut().set_field(
            EdictNumber::new(n),
            slot,
            Slot::from_f32(value),
        );
    }

    pub(crate) fn set_vector(&mut self, n: usize, name: &str, value: [f32; 3]) {
        let first = field_slot(name);
        let edicts = self.context.machine().edicts_mut();
        for (i, v) in value.into_iter().enumerate() {
            // A slot past the edict's fields is not written.
            let _written = edicts.set_field(
                EdictNumber::new(n),
                first.saturating_add(i),
                Slot::from_f32(v),
            );
        }
    }

    /// Sets a field to a raw value: a string, function or entity value.
    pub(crate) fn set_entity_raw(&mut self, n: usize, name: &str, value: i32) {
        let slot = field_slot(name);
        // A slot past the edict's fields is not written.
        let _written = self.context.machine().edicts_mut().set_field(
            EdictNumber::new(n),
            slot,
            Slot::from_i32(value),
        );
    }

    pub(crate) fn set_entity(&mut self, n: usize, name: &str, ent: usize) {
        let value = self.value_of(ent);
        let slot = field_slot(name);
        // A slot past the edict's fields is not written.
        let _written = self.context.machine().edicts_mut().set_field(
            EdictNumber::new(n),
            slot,
            Slot::from_i32(value),
        );
    }

    /// `(int)flags`.
    pub(crate) fn flags(&self, n: usize) -> i32 {
        float_to_int(self.float(n, "flags"))
    }

    pub(crate) fn set_flags(&mut self, n: usize, flags: i32) {
        self.set_float(n, "flags", int_to_float(flags));
    }

    pub(crate) fn global(&self, name: &str) -> Slot {
        global_by_name(name)
            .and_then(|v| i32::try_from(v.slot).ok())
            .and_then(|slot| self.context.machine_view().global(slot).ok())
            .unwrap_or(Slot::from_i32(0))
    }

    pub(crate) fn set_global(&mut self, name: &str, value: Slot) {
        if let Some(slot) = global_by_name(name).and_then(|v| i32::try_from(v.slot).ok())
            && self.context.machine().set_global(slot, value).is_err()
        {
            // A system global of a progs that loaded is always in range.
        }
    }

    /// The value of the server setting `setting` (PHYS-12.2).
    pub(crate) fn setting(&self, setting: Setting) -> f32 {
        self.sim
            .mock
            .engine()
            .cvars
            .value(setting.name())
            .unwrap_or(0.0)
    }

    /// The edict `self` names.
    pub(crate) fn self_edict(&self) -> usize {
        self.edict_of(self.global("self").as_i32())
    }

    pub(crate) fn set_return(&mut self, value: f32) -> Result<(), RunError> {
        if self
            .context
            .machine()
            .set_global(i32::from(PROGS_OFS_RETURN), Slot::from_f32(value))
            .is_err()
        {
            // OFS_RETURN is always a global.
        }
        Ok(())
    }

    /// One draw of the engine's random source, id's `rand()`.
    pub(crate) fn rand(&mut self) -> u32 {
        self.sim.mock.engine_mut().rng.next_rand()
    }

    // ---- the world builtins (WORLD-8) ----

    /// `PF_traceline` (WORLD-8.4, BUILTIN-14.1, BUILTIN-14.2).
    fn traceline(&mut self) -> Result<(), RunError> {
        let v1 = parm_vector(&self.context, 0);
        let v2 = parm_vector(&self.context, 1);
        let kind = float_to_int(parm_float(&self.context, 2));
        let pass = parm_edict(&self.context, 3)?;
        let trace = self.trace((v1, [0.0; 3], [0.0; 3], v2), MoveKind::from_int(kind), pass)?;
        let truth = |b: bool| Slot::from_f32(if b { 1.0 } else { 0.0 });
        self.set_global("trace_allsolid", truth(trace.allsolid));
        self.set_global("trace_startsolid", truth(trace.startsolid));
        self.set_global("trace_fraction", Slot::from_f32(trace.fraction));
        self.set_global("trace_inwater", truth(trace.inwater));
        self.set_global("trace_inopen", truth(trace.inopen));
        self.set_global_vector("trace_endpos", trace.endpos);
        self.set_global_vector("trace_plane_normal", trace.plane.normal);
        self.set_global("trace_plane_dist", Slot::from_f32(trace.plane.dist));
        let ent = self.value_of(trace.ent.unwrap_or(0));
        self.set_global("trace_ent", Slot::from_i32(ent));
        Ok(())
    }

    /// `PF_checkclient` (WORLD-8.7, BUILTIN-14.3): the client `self` might see, or the world.
    fn checkclient(&mut self) -> Result<(), RunError> {
        let clock = self.clock();
        let state = self.sim.mock.engine().checkclient;
        if clock - state.last_check_time >= BUILTIN_CHECKCLIENT_INTERVAL {
            let chosen = self.new_check_client(state.last_check);
            let check = &mut self.sim.mock.engine_mut().checkclient;
            check.last_check = chosen;
            check.last_check_time = clock;
        }
        let chosen = usize::try_from(self.sim.mock.engine().checkclient.last_check).unwrap_or(0);
        if self.is_free(chosen) || self.float(chosen, "health") <= 0.0 {
            return self.set_return_entity(0);
        }
        // "if current entity can't possibly see the check entity, return 0": the leaf at
        // self's eye, numbered from the first after leaf 0, must be in the stored PVS.
        let me = self.self_edict();
        let view = self.eye(me);
        let visible = self
            .sim
            .messages
            .vis
            .point_in_leaf(view)
            .and_then(|leaf| leaf.checked_sub(1))
            .is_some_and(|l| {
                self.sim
                    .checkpvs
                    .get(l >> 3)
                    .is_some_and(|byte| byte & (1 << (l & 7)) != 0)
            });
        self.set_return_entity(if visible { chosen } else { 0 })
    }

    /// `PF_aim (ent, speed)` (WORLD-8.8): `v_forward`, or the direction toward the edict to
    /// `DAMAGE_AIM` closest to it within `sv_aim`'s cone that a straight line from 20 above
    /// `ent`'s origin reaches.
    #[expect(
        clippy::float_cmp,
        clippy::float_cmp_const,
        reason = "spec WORLD-8.8: id compares takedamage and team exactly as floats"
    )]
    fn aim(&mut self) -> Result<(), RunError> {
        let ent = parm_edict(&self.context, 0)?;
        let mut start = self.vector(ent, "origin");
        start[2] += 20.0;
        let forward = self.global_vector("v_forward");
        let end = [
            start[0] + 2048.0 * forward[0],
            start[1] + 2048.0 * forward[1],
            start[2] + 2048.0 * forward[2],
        ];
        // "try sending a trace straight"
        let teamplay = self.setting(Setting::Teamplay) != 0.0;
        let team = self.float(ent, "team");
        let trace = self.trace((start, [0.0; 3], [0.0; 3], end), MoveKind::Normal, ent)?;
        if let Some(hit) = trace.ent
            && self.float(hit, "takedamage") == DAMAGE_AIM
            && (!teamplay || team <= 0.0 || team != self.float(hit, "team"))
        {
            return self.set_return_vector(forward);
        }
        // "try all possible entities"
        let mut bestdist = self.setting(Setting::Aim);
        let mut bestent = None;
        let edicts = self.context.machine_view().edicts().num_edicts();
        for check in 1..edicts {
            if self.float(check, "takedamage") != DAMAGE_AIM || check == ent {
                continue;
            }
            // "don't aim at teammate"
            if teamplay && team > 0.0 && team == self.float(check, "team") {
                continue;
            }
            let origin = self.vector(check, "origin");
            let mins = self.vector(check, "mins");
            let maxs = self.vector(check, "maxs");
            // `origin[j] + 0.5*(mins[j] + maxs[j])`, in double (PHYS-9.7's rule).
            let mut end = [0.0_f32; 3];
            for (((e, o), lo), hi) in end.iter_mut().zip(origin).zip(mins).zip(maxs) {
                *e = double_to_float(f64::from(o) + 0.5 * f64::from(lo + hi));
            }
            let (dir, _) =
                vector_normalize([end[0] - start[0], end[1] - start[1], end[2] - start[2]]);
            let dist = dot(dir, forward);
            // "to far to turn"
            if dist < bestdist {
                continue;
            }
            let trace = self.trace((start, [0.0; 3], [0.0; 3], end), MoveKind::Normal, ent)?;
            if trace.ent == Some(check) {
                bestdist = dist;
                bestent = Some(check);
            }
        }
        let Some(best) = bestent else {
            return self.set_return_vector(forward);
        };
        let to = self.vector(best, "origin");
        let from = self.vector(ent, "origin");
        let dir = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
        let dist = dot(dir, forward);
        let mut end = forward.map(|c| c * dist);
        end[2] = dir[2];
        let (end, _) = vector_normalize(end);
        self.set_return_vector(end)
    }

    /// The system global vector `name`.
    fn global_vector(&self, name: &str) -> [f32; 3] {
        let Some(first) = global_by_name(name).and_then(|v| i32::try_from(v.slot).ok()) else {
            return [0.0; 3];
        };
        core::array::from_fn(|i| {
            let slot = first.saturating_add(i32::try_from(i).unwrap_or(0));
            self.context
                .machine_view()
                .global(slot)
                .map_or(0.0, Slot::as_f32)
        })
    }

    /// Returns the vector `value`.
    fn set_return_vector(&mut self, value: [f32; 3]) -> Result<(), RunError> {
        for (i, v) in (0..).zip(value) {
            if self
                .context
                .machine()
                .set_global(
                    i32::from(PROGS_OFS_RETURN).saturating_add(i),
                    Slot::from_f32(v),
                )
                .is_err()
            {
                // OFS_RETURN's three slots are always globals.
            }
        }
        Ok(())
    }

    /// `PF_newcheckclient` (WORLD-8.7): from after `check`, the next client edict that is not
    /// free, has health above 0 and no `FL_NOTARGET`, or `check` itself when none other does;
    /// its PVS is stored.
    fn new_check_client(&mut self, check: i32) -> i32 {
        let maxclients =
            i32::try_from(self.context.machine_view().edicts().maxclients()).unwrap_or(1);
        let check = check.max(1).min(maxclients);
        let mut i = if check == maxclients {
            1
        } else {
            check.saturating_add(1)
        };
        loop {
            if i == maxclients.saturating_add(1) {
                i = 1;
            }
            if i == check {
                break;
            }
            let n = usize::try_from(i).unwrap_or(0);
            let skip = self.is_free(n)
                || self.float(n, "health") <= 0.0
                || self.flags(n) & BUILTIN_FL_NOTARGET != 0;
            if !skip {
                break;
            }
            i = i.saturating_add(1);
        }
        let n = usize::try_from(i).unwrap_or(0);
        let org = self.eye(n);
        // A point in no leaf, where id stops the engine, or a PVS that does not read, sees
        // nothing.
        let vis = &self.sim.messages.vis;
        self.sim.checkpvs = vis
            .point_in_leaf(org)
            .and_then(|leaf| vis.leaf_row(leaf))
            .map(<[u8]>::to_vec)
            .unwrap_or_default();
        i
    }

    /// Edict `n`'s `origin + view_ofs`.
    fn eye(&self, n: usize) -> [f32; 3] {
        let origin = self.vector(n, "origin");
        let view_ofs = self.vector(n, "view_ofs");
        [
            origin[0] + view_ofs[0],
            origin[1] + view_ofs[1],
            origin[2] + view_ofs[2],
        ]
    }

    /// Returns the entity value of edict `n`.
    fn set_return_entity(&mut self, n: usize) -> Result<(), RunError> {
        let value = self.value_of(n);
        if self
            .context
            .machine()
            .set_global(i32::from(PROGS_OFS_RETURN), Slot::from_i32(value))
            .is_err()
        {
            // OFS_RETURN is always a global.
        }
        Ok(())
    }

    fn set_global_vector(&mut self, name: &str, value: [f32; 3]) {
        if let Some(first) = global_by_name(name).and_then(|v| i32::try_from(v.slot).ok()) {
            for (i, v) in (0..).zip(value) {
                if self
                    .context
                    .machine()
                    .set_global(first.saturating_add(i), Slot::from_f32(v))
                    .is_err()
                {
                    // A system global of a progs that loaded is always in range.
                }
            }
        }
    }

    /// `PF_droptofloor` (WORLD-8.5, BUILTIN-15.3).
    fn droptofloor(&mut self) -> Result<(), RunError> {
        let ent = self.self_edict();
        let origin = self.vector(ent, "origin");
        let [x, y, z] = origin;
        let end = [x, y, z - DROP_DEPTH];
        let line = (
            origin,
            self.vector(ent, "mins"),
            self.vector(ent, "maxs"),
            end,
        );
        let trace = self.trace(line, MoveKind::Normal, ent)?;
        #[expect(
            clippy::float_cmp,
            reason = "spec WORLD-8.5: id tests trace.fraction == 1"
        )]
        let missed = trace.fraction == 1.0;
        if missed || trace.allsolid {
            return self.set_return(0.0);
        }
        self.set_vector(ent, "origin", trace.endpos);
        self.link(ent, false)?;
        let flags = self.flags(ent) | FL_ONGROUND;
        self.set_flags(ent, flags);
        self.set_entity(ent, "groundentity", trace.ent.unwrap_or(0));
        self.set_return(1.0)
    }
}

impl Host for SimCall<'_, '_, '_> {
    fn collision(&self) -> &Collision {
        &self.sim.collision
    }

    fn collision_mut(&mut self) -> &mut Collision {
        &mut self.sim.collision
    }

    fn body(&self, n: usize) -> Option<Body> {
        let edicts = self.context.machine_view().edicts();
        let free = edicts.is_free(EdictNumber::new(n))?;
        #[expect(
            clippy::float_cmp_const,
            reason = "spec WORLD-1.4: id tests movetype == MOVETYPE_PUSH"
        )]
        let pusher = self.float(n, "movetype") == MOVETYPE_PUSH;
        Some(Body {
            free,
            solid: Solid::from_float(self.float(n, "solid")),
            pusher,
            modelindex: self.float(n, "modelindex"),
            origin: self.vector(n, "origin"),
            mins: self.vector(n, "mins"),
            maxs: self.vector(n, "maxs"),
            absmin: self.vector(n, "absmin"),
            absmax: self.vector(n, "absmax"),
            size: self.vector(n, "size"),
            flags: self.flags(n),
            owner: Some(self.entity(n, "owner")),
            touch: field(&self.context, n, "touch").map_or(0, Slot::as_i32),
        })
    }

    fn set_abs(&mut self, n: usize, absmin: [f32; 3], absmax: [f32; 3]) {
        self.set_vector(n, "absmin", absmin);
        self.set_vector(n, "absmax", absmax);
    }

    fn touch(&mut self, trigger: usize, other: usize) -> Result<(), CollisionError> {
        let function = field(&self.context, trigger, "touch").map_or(0, Slot::as_i32);
        let old_self = self.global("self");
        let old_other = self.global("other");
        let trigger_value = self.value_of(trigger);
        let other_value = self.value_of(other);
        self.set_global("self", Slot::from_i32(trigger_value));
        self.set_global("other", Slot::from_i32(other_value));
        let time = double_to_float(self.sim.mock.engine().clock);
        self.set_global("time", Slot::from_f32(time));
        let result = self.context.execute_program(self.sim, function);
        self.set_global("self", old_self);
        self.set_global("other", old_other);
        result.map_err(|e| CollisionError::Touch(Box::new(e)))
    }
}

/// The slot of the system field `name` (EDICT-3.1).
pub(crate) fn field_slot(name: &str) -> usize {
    field_by_name(name).map_or(0, |var| var.slot)
}

fn field(vm: &Vm<'_, '_, '_>, n: usize, name: &str) -> Option<Slot> {
    vm.machine_view()
        .edicts()
        .field(EdictNumber::new(n), field_slot(name))
}

/// Parameter `n`'s raw bits.
pub(crate) fn parm_raw(context: &Vm<'_, '_, '_>, n: u16) -> i32 {
    parm(context, n).as_i32()
}

fn parm(context: &Vm<'_, '_, '_>, n: u16) -> Slot {
    ofs_parm(n)
        .and_then(|slot| context.machine_view().global(i32::from(slot)).ok())
        .unwrap_or(Slot::from_i32(0))
}

pub(crate) fn parm_float(context: &Vm<'_, '_, '_>, n: u16) -> f32 {
    parm(context, n).as_f32()
}

pub(crate) fn parm_vector(context: &Vm<'_, '_, '_>, n: u16) -> [f32; 3] {
    let first = ofs_parm(n).map_or(0, i32::from);
    [0, 1, 2].map(|i| {
        context
            .machine_view()
            .global(first.saturating_add(i))
            .map_or(0.0, Slot::as_f32)
    })
}

/// Parameter `n` as an edict, `G_EDICT`: an error for a value that is not one.
fn parm_edict(context: &Vm<'_, '_, '_>, n: u16) -> Result<usize, RunError> {
    let value = parm(context, n).as_i32();
    context
        .machine_view()
        .edicts()
        .layout()
        .edict_number(EntityRef::from_bits(value))
        .map(EdictNumber::index)
        .ok_or_else(|| {
            context.run_error(ErrorKind::Builtin {
                message: "invalid entity".to_owned(),
            })
        })
}
