// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_cmds.c, PF_setorigin, PF_setsize, PF_setmodel, PF_traceline, PF_droptofloor, PF_walkmove, PF_checkbottom, PF_pointcontents
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_edict.c, ED_Free

//! The server's builtins: `qc-builtins`' for the engine state it keeps, and the world builtins
//! on this crate's collision (WORLD-8) and movement (MOVE).

use qc_builtins::mock::ModelBounds;
use qc_builtins::{MockBuiltins, MockEngine};
use qc_edict::system::{field_by_name, global_by_name};
use qc_types::{EntityRef, PROGS_OFS_RETURN, Slot, ofs_parm};
use qc_vm::{Builtins, Context, ErrorKind, RunError};
use qs_bsp::{Bsp, BspLoader};
use qs_mdl::Sprite;

use crate::collision::{Body, Collision, CollisionError, Host, MoveKind, Solid, sv_move};

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
/// `precache_model2`.
const PRECACHE_MODEL2: u32 = 75;
/// `end[2] -= 256` in `PF_droptofloor` (WORLD-8.5).
const DROP_DEPTH: f32 = 256.0;
/// `MOVETYPE_PUSH` (WORLD-1.4).
const MOVETYPE_PUSH: f32 = 7.0;
/// `FL_ONGROUND`.
pub(crate) const FL_ONGROUND: i32 = 512;

/// Reads a model file the progs precaches, such as `maps/b_bh25.bsp`.
pub type ModelFiles = Box<dyn Fn(&str) -> Option<Vec<u8>>>;

/// The builtins a world runs its progs with.
pub struct SimBuiltins {
    /// The engine state builtins read and write, and every builtin but the world's.
    pub(crate) mock: MockBuiltins<MockEngine>,
    /// The world's collision.
    pub(crate) collision: Collision,
    files: ModelFiles,
}

impl SimBuiltins {
    /// The builtins of the map `map`, named `name`, with its brush models' bounds for
    /// `setmodel` (WORLD-8.2), reading other models with `files`.
    pub(crate) fn new(engine: MockEngine, name: &str, map: &Bsp<'_>, files: ModelFiles) -> Self {
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

impl Builtins for SimBuiltins {
    fn call(&mut self, number: u32, context: &mut Context<'_, '_>) -> Result<(), RunError> {
        match number {
            // WORLD-8.1: the mock sets the fields; the world links.
            SETORIGIN | SETMODEL | SETSIZE => {
                self.mock.call(number, context)?;
                let ent = parm_edict(context, 0)?;
                SimCall::new(context, self).link(ent, false)
            }
            // WORLD-8.3: ED_Free unlinks.
            REMOVE | MAKESTATIC => {
                self.mock.call(number, context)?;
                let ent = parm_edict(context, 0)?;
                self.collision.unlink(ent);
                Ok(())
            }
            PRECACHE_MODEL | PRECACHE_MODEL2 => {
                let before = self.mock.engine().model_precache.len();
                self.mock.call(number, context)?;
                let list = &self.mock.engine().model_precache;
                if list.len() > before
                    && let Some(name) = list.last().cloned()
                {
                    self.loaded(before, &name);
                }
                Ok(())
            }
            TRACELINE => SimCall::new(context, self).traceline(),
            DROPTOFLOOR => SimCall::new(context, self).droptofloor(),
            WALKMOVE => SimCall::new(context, self).walkmove(),
            MOVETOGOAL => SimCall::new(context, self).move_to_goal(),
            CHECKBOTTOM => {
                let ent = parm_edict(context, 0)?;
                let mut call = SimCall::new(context, self);
                let bottom = call.bottom(ent)?;
                call.set_return(if bottom { 1.0 } else { 0.0 })
            }
            POINTCONTENTS => {
                let p = parm_vector(context, 0);
                let mut call = SimCall::new(context, self);
                let contents = call.sim.collision.point_contents(p).map_err(|e| call.error(e))?;
                call.set_return(f32::from(i16::try_from(contents).unwrap_or(i16::MIN)))
            }
            _ => self.mock.call(number, context),
        }
    }

    fn engine_string(&self, value: i32) -> Option<&[u8]> {
        self.mock.engine_string(value)
    }
}

/// One builtin call: the VM's context and the server's builtins, which together answer the
/// world's queries (`Host`).
pub(crate) struct SimCall<'c, 'a, 'p> {
    pub(crate) context: &'c mut Context<'a, 'p>,
    pub(crate) sim: &'c mut SimBuiltins,
}

impl<'c, 'a, 'p> SimCall<'c, 'a, 'p> {
    pub(crate) fn new(context: &'c mut Context<'a, 'p>, sim: &'c mut SimBuiltins) -> Self {
        Self { context, sim }
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
        crate::collision::link_edict(self, ent, touch).map_err(|e| self.error(e))
    }

    /// `SV_Move`, ignoring `pass`.
    pub(crate) fn trace(
        &self,
        line: ([f32; 3], [f32; 3], [f32; 3], [f32; 3]),
        kind: MoveKind,
        pass: usize,
    ) -> Result<crate::collision::Trace, RunError> {
        sv_move(self, line, kind, Some(pass)).map_err(|e| self.error(e))
    }

    // ---- fields and globals ----

    pub(crate) fn float(&self, n: usize, name: &str) -> f32 {
        field(self.context, n, name).map_or(0.0, Slot::as_f32)
    }

    pub(crate) fn vector(&self, n: usize, name: &str) -> [f32; 3] {
        let first = field_slot(name);
        let edicts = self.context.machine_view().edicts();
        [0, 1, 2].map(|i| {
            edicts
                .field(n, first.saturating_add(i))
                .map_or(0.0, Slot::as_f32)
        })
    }

    /// The edict an entity field names, id's `PROG_TO_EDICT`; the world for any value that
    /// is not an edict.
    pub(crate) fn entity(&self, n: usize, name: &str) -> usize {
        let value = field(self.context, n, name).map_or(0, Slot::as_i32);
        self.edict_of(value)
    }

    pub(crate) fn edict_of(&self, value: i32) -> usize {
        let layout = self.context.machine_view().edicts().layout();
        layout
            .edict_number(EntityRef(value))
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(0)
    }

    pub(crate) fn value_of(&self, n: usize) -> i32 {
        let size = self.context.machine_view().edicts().layout().edict_size();
        i32::try_from(n).ok().and_then(|n| n.checked_mul(size)).unwrap_or(0)
    }

    pub(crate) fn set_float(&mut self, n: usize, name: &str, value: f32) {
        let slot = field_slot(name);
        self.context
            .machine()
            .edicts_mut()
            .set_field(n, slot, Slot::from_f32(value));
    }

    pub(crate) fn set_vector(&mut self, n: usize, name: &str, value: [f32; 3]) {
        let first = field_slot(name);
        let edicts = self.context.machine().edicts_mut();
        for (i, v) in value.into_iter().enumerate() {
            edicts.set_field(n, first.saturating_add(i), Slot::from_f32(v));
        }
    }

    pub(crate) fn set_entity(&mut self, n: usize, name: &str, ent: usize) {
        let value = self.value_of(ent);
        let slot = field_slot(name);
        self.context
            .machine()
            .edicts_mut()
            .set_field(n, slot, Slot::from_i32(value));
    }

    /// `(int)flags`.
    pub(crate) fn flags(&self, n: usize) -> i32 {
        qc_builtins::math::float_to_int(self.float(n, "flags"))
    }

    pub(crate) fn set_flags(&mut self, n: usize, flags: i32) {
        self.set_float(n, "flags", qc_builtins::math::int_to_f32_public(flags));
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
        qc_builtins::Engine::next_rand(self.sim.mock.engine_mut())
    }

    // ---- the world builtins (WORLD-8) ----

    /// `PF_traceline` (WORLD-8.4, BUILTIN-14.1, BUILTIN-14.2).
    fn traceline(&mut self) -> Result<(), RunError> {
        let v1 = parm_vector(self.context, 0);
        let v2 = parm_vector(self.context, 1);
        let kind = qc_builtins::math::float_to_int(parm_float(self.context, 2));
        let pass = parm_edict(self.context, 3)?;
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
        let line = (origin, self.vector(ent, "mins"), self.vector(ent, "maxs"), end);
        let trace = self.trace(line, MoveKind::Normal, ent)?;
        #[expect(clippy::float_cmp, reason = "spec WORLD-8.5: id tests trace.fraction == 1")]
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
        let free = edicts.is_free(n)?;
        #[expect(clippy::float_cmp, reason = "spec WORLD-1.4: id tests movetype == MOVETYPE_PUSH")]
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
            touch: field(self.context, n, "touch").map_or(0, Slot::as_i32),
        })
    }

    fn set_abs(&mut self, n: usize, absmin: [f32; 3], absmax: [f32; 3]) {
        self.set_vector(n, "absmin", absmin);
        self.set_vector(n, "absmax", absmax);
    }

    fn touch(&mut self, trigger: usize, other: usize) -> Result<(), CollisionError> {
        let function = field(self.context, trigger, "touch").map_or(0, Slot::as_i32);
        let old_self = self.global("self");
        let old_other = self.global("other");
        let trigger_value = self.value_of(trigger);
        let other_value = self.value_of(other);
        self.set_global("self", Slot::from_i32(trigger_value));
        self.set_global("other", Slot::from_i32(other_value));
        let time = qc_builtins::math::f64_to_f32_public(self.sim.mock.engine().clock);
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

fn field(context: &Context<'_, '_>, n: usize, name: &str) -> Option<Slot> {
    context.machine_view().edicts().field(n, field_slot(name))
}

fn parm(context: &Context<'_, '_>, n: u16) -> Slot {
    ofs_parm(n)
        .and_then(|slot| context.machine_view().global(i32::from(slot)).ok())
        .unwrap_or(Slot::from_i32(0))
}

pub(crate) fn parm_float(context: &Context<'_, '_>, n: u16) -> f32 {
    parm(context, n).as_f32()
}

pub(crate) fn parm_vector(context: &Context<'_, '_>, n: u16) -> [f32; 3] {
    let first = ofs_parm(n).map_or(0, i32::from);
    [0, 1, 2].map(|i| {
        context
            .machine_view()
            .global(first.saturating_add(i))
            .map_or(0.0, Slot::as_f32)
    })
}

/// Parameter `n` as an edict, `G_EDICT`: an error for a value that is not one.
fn parm_edict(context: &Context<'_, '_>, n: u16) -> Result<usize, RunError> {
    let value = parm(context, n).as_i32();
    context
        .machine_view()
        .edicts()
        .layout()
        .edict_number(EntityRef(value))
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| {
            context.run_error(ErrorKind::Builtin {
                message: "invalid entity".to_owned(),
            })
        })
}
