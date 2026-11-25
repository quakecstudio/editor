// SPDX-License-Identifier: GPL-3.0-or-later

//! Firing an entity and tracing what the program does about it (L-76).
//!
//! [`World::fire`] runs an entity's `use`, or its `touch`, as the game would, then the server's
//! frames, and records what ran because of it: each `use`, `touch`, `think` and `blocked` the
//! firing reached, each entity looked up by its `targetname`, removed or spawned, each
//! `centerprint` and sound, and the fields written. Nothing here knows a classname: what firing
//! means is the program's own code, so a mod's triggers are traced as id's are.
//!
//! Implements `docs/spec/fire.md` FIRE-1 to FIRE-3.

use std::collections::{BTreeMap, BTreeSet};

use qc_edict::system::global_by_name;
use qc_edict::{EdictEvent, EdictNumber, Tracking};
use qc_progs::Progs;
use qc_progs::opcode::Opcode;
use qc_types::{EntityRef, PROGS_OFS_RETURN, Slot, ofs_parm};
use qc_vm::Machine;
use qc_vm::convert::double_to_float;

use super::{CLIENT_EDICT, EdictChange, HostCommand, NamedValue, World, WorldError};
use crate::builtins::{SimCall, field_slot};
use crate::client::UserCmd;
use crate::scenario::{SCN_FRAMETIME, Seconds};

/// `find` in id's builtin table (BUILTIN-8).
const FIND: u32 = 18;
/// `sound` in id's builtin table (BUILTIN-12).
const SOUND: u32 = 8;
/// `centerprint` in id's builtin table (BUILTIN-11.2).
const CENTERPRINT: u32 = 73;
/// How long a trace runs when the caller gives no length, in seconds (FIRE-3.1).
pub const FIRE_SECONDS: Seconds = Seconds::new(10.0);
/// The longest a trace may run, in seconds (FIRE-3.1).
pub const FIRE_MAX_SECONDS: Seconds = Seconds::new(300.0);
/// The first parameter of a builtin.
const PARM0: u16 = 0;
/// The second.
const PARM1: u16 = 1;
/// The third.
const PARM2: u16 = 2;
/// The frame in which a client that connects in frame 1 enters the game (DIFF-16.1).
const ENTERED: usize = 4;

/// Which of an entity's functions fires it (FIRE-1.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FireBy {
    /// Its `use` when it has one, else its `touch`.
    #[default]
    Either,
    /// Its `use`, as another entity's targets are fired.
    Use,
    /// Its `touch`, as a player walking into it.
    Touch,
}

/// One of the four functions the engine and the program call an entity by (FIRE-2.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Callback {
    /// `use`.
    Use,
    /// `touch`.
    Touch,
    /// `think`.
    Think,
    /// `blocked`.
    Blocked,
}

/// One thing a firing did (FIRE-2).
#[derive(Clone, Debug, PartialEq)]
pub enum TraceEvent {
    /// A callback of an entity ran (FIRE-2.1).
    Ran {
        /// The `time` global then.
        time: f32,
        /// Which callback.
        callback: Callback,
        /// The entity, `self`.
        entity: usize,
        /// Its classname.
        classname: String,
        /// The function's name.
        function: String,
    },
    /// The program looked an entity up by its `targetname` (FIRE-2.3).
    Looked {
        /// The `time` global then.
        time: f32,
        /// `self` then: the entity whose targets these are.
        by: usize,
        /// The name looked for.
        name: String,
        /// The entity found, `None` when there was none, or no more.
        found: Option<usize>,
    },
    /// An entity was looked up by its `targetname`, only its `use` was read, it has none, and
    /// nothing of it was called: firing it does nothing (FIRE-2.4).
    Unanswered {
        /// The `time` global then.
        time: f32,
        /// `self` at the lookup.
        by: usize,
        /// The entity found.
        entity: usize,
        /// Its classname.
        classname: String,
    },
    /// An entity was removed (FIRE-2.5).
    Removed {
        /// The `time` global when the run that removed it began.
        time: f32,
        /// The entity.
        entity: usize,
        /// The classname it had.
        classname: String,
    },
    /// An entity was spawned (FIRE-2.5).
    Spawned {
        /// The `time` global when the run that spawned it began.
        time: f32,
        /// The entity.
        entity: usize,
        /// Its classname at the end of the frame.
        classname: String,
    },
    /// `centerprint` printed to a client (FIRE-2.6).
    Print {
        /// The `time` global then.
        time: f32,
        /// The text.
        text: String,
    },
    /// `sound` started a sample (FIRE-2.6).
    Sound {
        /// The `time` global then.
        time: f32,
        /// The entity it comes from.
        entity: usize,
        /// The sample.
        sample: String,
    },
    /// Fields of an entity the firing reached were written in a frame (FIRE-2.7).
    Changed {
        /// The server's time at the end of the frame.
        time: f32,
        /// The entity.
        entity: usize,
        /// The fields written, with their values at the end of the frame.
        fields: Vec<NamedValue>,
    },
}

/// Why a trace ended (FIRE-3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TraceEnd {
    /// No entity the firing reached has a think pending (FIRE-3.2).
    Quiet,
    /// The trace ran its whole length (FIRE-3.1).
    Length,
    /// The program asked the host for a command, such as `changelevel` (FIRE-3.3).
    Host(HostCommand),
    /// A function failed, with the error's text (FIRE-3.4).
    Error(String),
}

/// What a firing did (FIRE-2).
#[derive(Clone, Debug, PartialEq)]
pub struct FireTrace {
    /// The callback the entity was fired by.
    pub fired_by: Callback,
    /// What happened, in order.
    pub events: Vec<TraceEvent>,
    /// Why the trace ended.
    pub end: TraceEnd,
    /// The simulated seconds the trace ran after the firing.
    pub seconds: f64,
}

/// How to fire and how long to trace.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FireOptions {
    /// Which function fires the entity.
    pub by: FireBy,
    /// The longest the trace runs, clamped to [`FIRE_MAX_SECONDS`].
    pub length: Seconds,
    /// A frame's length.
    pub frametime: Seconds,
}

impl Default for FireOptions {
    fn default() -> Self {
        Self {
            by: FireBy::Either,
            length: FIRE_SECONDS,
            frametime: SCN_FRAMETIME,
        }
    }
}

/// Why an entity could not be fired (FIRE-1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FireError {
    /// The edict is not in use.
    NotInUse(usize),
    /// The entity has no function of the kind asked for (FIRE-1.3).
    NothingToFire {
        /// The edict.
        entity: usize,
        /// What was asked for.
        by: FireBy,
    },
}

impl core::fmt::Display for FireError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotInUse(n) => write!(f, "edict {n} is not in use"),
            Self::NothingToFire {
                entity,
                by: FireBy::Use,
            } => write!(f, "edict {entity} has no use function"),
            Self::NothingToFire {
                entity,
                by: FireBy::Touch,
            } => write!(f, "edict {entity} has no touch function"),
            Self::NothingToFire {
                entity,
                by: FireBy::Either,
            } => write!(f, "edict {entity} has no use and no touch function"),
        }
    }
}

impl core::error::Error for FireError {}

/// An event as the run records it, its strings and classnames not yet read.
#[derive(Clone, Debug)]
enum Raw {
    Ran {
        time: f32,
        callback: Callback,
        entity: usize,
        function: i32,
    },
    Looked {
        time: f32,
        by: usize,
        name: Slot,
        found: Option<usize>,
    },
    Unanswered {
        time: f32,
        by: usize,
        entity: usize,
    },
    Removed {
        time: f32,
        entity: usize,
    },
    Spawned {
        time: f32,
        entity: usize,
    },
    Print {
        time: f32,
        parts: Vec<Slot>,
    },
    Sound {
        time: f32,
        entity: usize,
        sample: Slot,
    },
}

/// An entity found by its `targetname`, and what the program did with it since (FIRE-2.4).
#[derive(Clone, Copy, Debug)]
struct Lookup {
    time: f32,
    by: usize,
    entity: usize,
    /// Its `use` was read and was no function.
    no_use: bool,
    /// Another field of it was read or addressed, or a function ran with it as `self`.
    answered: bool,
}

/// Where the globals and fields the tracer reads live.
#[derive(Clone, Copy, Debug)]
struct Slots {
    self_global: i32,
    other_global: i32,
    time_global: i32,
    /// `use`, `touch`, `think` and `blocked`, in the order FIRE-2.1 checks them.
    callbacks: [(Callback, usize); 4],
    targetname: usize,
}

impl Slots {
    fn new() -> Self {
        let global = |name: &str| {
            global_by_name(name)
                .and_then(|var| i32::try_from(var.slot).ok())
                .unwrap_or(0)
        };
        Self {
            self_global: global("self"),
            other_global: global("other"),
            time_global: global("time"),
            callbacks: [
                (Callback::Use, field_slot("use")),
                (Callback::Touch, field_slot("touch")),
                (Callback::Think, field_slot("think")),
                (Callback::Blocked, field_slot("blocked")),
            ],
            targetname: field_slot("targetname"),
        }
    }
}

/// The edict a global or a parameter names, `None` for a value that is no edict.
fn edict_at(machine: &Machine, slot: i32) -> Option<usize> {
    let value = machine.global(slot).ok()?;
    machine
        .edicts()
        .layout()
        .edict_number(EntityRef::from_bits(value.as_i32()))
        .map(EdictNumber::index)
}

/// Builtin parameter `n`.
fn parm(machine: &Machine, n: u16) -> Slot {
    ofs_parm(n)
        .and_then(|slot| machine.global(i32::from(slot)).ok())
        .unwrap_or(Slot::ZERO)
}

/// The recorder of a firing, which the frame's observer feeds (FIRE-2).
#[derive(Clone, Debug)]
pub(crate) struct Tracer {
    /// The entities the firing has reached (FIRE-2.2).
    affected: BTreeSet<usize>,
    /// Whether the run under way is one the firing reached.
    recording: bool,
    /// Whether the next statement is a function's first.
    entering: bool,
    /// The `time` global when the run under way began.
    run_time: f32,
    raw: Vec<Raw>,
    lookup: Option<Lookup>,
    /// Every edict event since the trace's events were last taken, for the frame's changes.
    events: Vec<EdictEvent>,
    slots: Slots,
}

impl Tracer {
    fn new(fired: usize) -> Self {
        Self {
            affected: BTreeSet::from([fired]),
            recording: false,
            entering: false,
            run_time: 0.0,
            raw: Vec::new(),
            lookup: None,
            events: Vec::new(),
            slots: Slots::new(),
        }
    }

    fn time(&self, machine: &Machine) -> f32 {
        machine
            .global(self.slots.time_global)
            .map_or(0.0, Slot::as_f32)
    }

    /// A run from the frame begins: it is recorded when `self` or `other` is an entity the
    /// firing reached (FIRE-2.2).
    pub(crate) fn begin_run(&mut self, machine: &mut Machine) {
        self.events.extend(machine.edicts_mut().take_events());
        let reached =
            |slot| edict_at(machine, slot).is_some_and(|n| n != 0 && self.affected.contains(&n));
        self.recording = reached(self.slots.self_global) || reached(self.slots.other_global);
        self.entering = false;
        self.lookup = None;
        self.run_time = self.time(machine);
    }

    /// The run from the frame ended: the entities a recorded run wrote or spawned are reached
    /// from now on (FIRE-2.2).
    pub(crate) fn end_run(&mut self, machine: &mut Machine) {
        self.close_lookup();
        let events = machine.edicts_mut().take_events();
        for event in &events {
            match *event {
                EdictEvent::Allocated(n) if self.recording => {
                    self.affected.insert(n.index());
                    self.raw.push(Raw::Spawned {
                        time: self.run_time,
                        entity: n.index(),
                    });
                }
                // An edict taken again by a run the firing did not reach is a new entity.
                EdictEvent::Allocated(n) => {
                    self.affected.remove(&n.index());
                }
                EdictEvent::Freed(n) => {
                    if self.recording {
                        self.raw.push(Raw::Removed {
                            time: self.run_time,
                            entity: n.index(),
                        });
                    }
                    self.affected.remove(&n.index());
                }
                EdictEvent::Written(n, _) if self.recording && n.index() != 0 => {
                    self.affected.insert(n.index());
                }
                EdictEvent::Written(..) => {}
            }
        }
        self.events.extend(events);
        self.recording = false;
    }

    /// `PR_ExecuteProgram` was entered: the next statement is a function's first.
    pub(crate) const fn run(&mut self) {
        self.entering = self.recording;
    }

    /// Before a statement: a function's first statement records the callback it is
    /// (FIRE-2.1).
    pub(crate) fn before(&mut self, machine: &Machine, function: i32) {
        if !core::mem::take(&mut self.entering) {
            return;
        }
        let Some(entity) = edict_at(machine, self.slots.self_global) else {
            return;
        };
        if let Some(lookup) = &mut self.lookup
            && lookup.entity == entity
        {
            lookup.answered = true;
        }
        let edicts = machine.edicts();
        let callback = self.slots.callbacks.iter().find(|&&(_, slot)| {
            edicts
                .field(EdictNumber::new(entity), slot)
                .is_some_and(|value| value.as_i32() == function)
        });
        if let Some(&(callback, _)) = callback {
            self.raw.push(Raw::Ran {
                time: self.time(machine),
                callback,
                entity,
                function,
            });
        }
    }

    /// After a statement (FIRE-2.3, FIRE-2.4, FIRE-2.6).
    pub(crate) fn step(
        &mut self,
        machine: &Machine,
        progs: &Progs<'_>,
        statement: i32,
        opcode: u16,
        builtin: Option<u32>,
    ) {
        if !self.recording {
            return;
        }
        match builtin {
            Some(FIND) => self.found(machine),
            Some(CENTERPRINT) => {
                let parts = (PARM1..)
                    .take(machine.argc().saturating_sub(1))
                    .map(|n| parm(machine, n))
                    .collect();
                self.raw.push(Raw::Print {
                    time: self.time(machine),
                    parts,
                });
            }
            Some(SOUND) => {
                if let Some(entity) = ofs_parm(PARM0).and_then(|s| edict_at(machine, i32::from(s)))
                {
                    self.raw.push(Raw::Sound {
                        time: self.time(machine),
                        entity,
                        sample: parm(machine, PARM2),
                    });
                }
            }
            Some(_) => {}
            None => self.stepped(machine, progs, statement, opcode),
        }
    }

    /// `find` returned: a lookup by `targetname` is recorded (FIRE-2.3).
    fn found(&mut self, machine: &Machine) {
        let field = usize::try_from(parm(machine, PARM1).as_i32()).ok();
        if field != Some(self.slots.targetname) {
            return;
        }
        self.close_lookup();
        let time = self.time(machine);
        let by = edict_at(machine, self.slots.self_global).unwrap_or(0);
        // BUILTIN-8.3: the world when no edict matches.
        let found = edict_at(machine, i32::from(PROGS_OFS_RETURN)).filter(|&n| n != 0);
        self.raw.push(Raw::Looked {
            time,
            by,
            name: parm(machine, PARM2),
            found,
        });
        self.lookup = found.map(|entity| Lookup {
            time,
            by,
            entity,
            no_use: false,
            answered: false,
        });
    }

    /// A statement of the program ran: a call enters a function, and a read of the entity
    /// last looked up says what the program did with it (FIRE-2.4).
    fn stepped(&mut self, machine: &Machine, progs: &Progs<'_>, statement: i32, opcode: u16) {
        let Some(opcode) = Opcode::from_number(u32::from(opcode)) else {
            return;
        };
        match opcode {
            Opcode::Call0
            | Opcode::Call1
            | Opcode::Call2
            | Opcode::Call3
            | Opcode::Call4
            | Opcode::Call5
            | Opcode::Call6
            | Opcode::Call7
            | Opcode::Call8 => self.entering = true,
            Opcode::LoadF
            | Opcode::LoadV
            | Opcode::LoadS
            | Opcode::LoadEnt
            | Opcode::LoadFld
            | Opcode::LoadFnc
            | Opcode::Address => {
                let Some(lookup) = &mut self.lookup else {
                    return;
                };
                let Some(read) = usize::try_from(statement)
                    .ok()
                    .and_then(|index| progs.statements().get(index))
                else {
                    return;
                };
                if edict_at(machine, read.a) != Some(lookup.entity) {
                    return;
                }
                let field = machine
                    .global(read.b)
                    .ok()
                    .and_then(|slot| usize::try_from(slot.as_i32()).ok());
                let (_, use_slot) = self.slots.callbacks[0];
                let function = machine.global(read.c).map_or(0, Slot::as_i32);
                if opcode == Opcode::LoadFnc && field == Some(use_slot) && function == 0 {
                    lookup.no_use = true;
                } else {
                    lookup.answered = true;
                }
            }
            _ => {}
        }
    }

    /// The entity last looked up is done with: one whose `use` was read and was none, and of
    /// which nothing else was read or called, answered nothing (FIRE-2.4).
    fn close_lookup(&mut self) {
        if let Some(lookup) = self.lookup.take()
            && lookup.no_use
            && !lookup.answered
        {
            self.raw.push(Raw::Unanswered {
                time: lookup.time,
                by: lookup.by,
                entity: lookup.entity,
            });
        }
    }
}

impl World {
    /// Brings the client into the game as a host does (DIFF-16.1, PHYS-10.7): it connects in
    /// frame 1 and answers each signon stage in the next frame, so the player is in the game
    /// after frame 4, where the program's `PutClientInServer` puts it.
    ///
    /// # Errors
    ///
    /// [`WorldError::Frame`] when a frame fails.
    pub fn enter(&mut self, progs: &Progs<'_>, frametime: Seconds) -> Result<(), WorldError> {
        for frame in 1..=ENTERED {
            match frame {
                1 => self.connect_next_frame(),
                2 => self.client_command("prespawn"),
                3 => {
                    self.client_command("name \"player\"\n");
                    self.client_command("color 0 0\n");
                    self.client_command("spawn ");
                }
                _ => self.client_command("begin"),
            }
            self.set_command(UserCmd::default());
            self.frame(progs, frametime.get())?;
            drop(self.take_messages());
        }
        Ok(())
    }

    /// Fires edict `n` and traces what the program does about it (FIRE-1 to FIRE-3): its
    /// `use` or its `touch` runs with the player as `other` and as the program's `activator`,
    /// then the server's frames, until nothing the firing reached has a think pending, the
    /// length is up, the program asks the host for a command, or a function fails.
    ///
    /// # Errors
    ///
    /// [`FireError`] when the edict is not in use or has no function to fire it by.
    pub fn fire(
        &mut self,
        progs: &Progs<'_>,
        n: usize,
        options: FireOptions,
    ) -> Result<FireTrace, FireError> {
        if !self.in_use(n) {
            return Err(FireError::NotInUse(n));
        }
        let function_of = |callback: &str| self.field(n, callback).as_i32();
        let (fired_by, function) = match options.by {
            FireBy::Use => (Callback::Use, function_of("use")),
            FireBy::Touch => (Callback::Touch, function_of("touch")),
            FireBy::Either => match function_of("use") {
                0 => (Callback::Touch, function_of("touch")),
                function => (Callback::Use, function),
            },
        };
        if function == 0 {
            return Err(FireError::NothingToFire {
                entity: n,
                by: options.by,
            });
        }
        // The classnames now, for an entity removed before its event is read.
        let mut classnames: BTreeMap<usize, String> = (0..self.num_edicts())
            .filter(|&edict| self.in_use(edict))
            .map(|edict| (edict, self.classname(progs, edict)))
            .collect();
        self.track_changes(Tracking::On);
        drop(self.machine.edicts_mut().take_events());
        self.builtins.tracer = Some(Tracer::new(n));

        // FIRE-1.4: the player when one is in the game, else the world.
        let activator = if self.in_use(CLIENT_EDICT) && self.builtins.messages.client.active {
            CLIENT_EDICT
        } else {
            0
        };
        let mut events = Vec::new();
        let mut end = None;
        {
            let mut call = SimCall::frame(&mut self.machine, progs, &mut self.builtins);
            call.set_time(call.clock());
            let (this, other) = (call.value_of(n), call.value_of(activator));
            call.set_global("self", Slot::from_i32(this));
            call.set_global("other", Slot::from_i32(other));
            // FIRE-1.4: `activator` is the program's own global, set when it has one.
            if let Some(slot) = progs
                .ed_find_global(b"activator")
                .and_then(|def| i32::try_from(def.ofs).ok())
                && call
                    .context
                    .machine()
                    .set_global(slot, Slot::from_i32(other))
                    .is_err()
            {
                // A definition's offset is in the globals.
            }
            if let Err(error) = call.run(function) {
                end = Some(TraceEnd::Error(error.to_string()));
            }
        }
        self.take_trace(progs, &mut classnames, &mut events);

        let length = options.length.get().clamp(0.0, FIRE_MAX_SECONDS.get());
        let frametime = options.frametime.get();
        let mut seconds = 0.0;
        let end = loop {
            if let Some(end) = end.take() {
                break end;
            }
            // FIRE-3.3: what the program left in the command buffer is read as a frame reads it.
            self.execute_console(progs);
            if let Some(command) = self.take_host_command() {
                break TraceEnd::Host(command);
            }
            if self.fire_is_quiet() {
                break TraceEnd::Quiet;
            }
            if seconds >= length || frametime <= 0.0 {
                break TraceEnd::Length;
            }
            if let Err(error) = self.frame(progs, frametime) {
                end = Some(TraceEnd::Error(error.to_string()));
            }
            drop(self.take_messages());
            seconds += frametime;
            self.take_trace(progs, &mut classnames, &mut events);
        };
        self.builtins.tracer = None;
        self.track_changes(Tracking::Off);
        Ok(FireTrace {
            fired_by,
            events,
            end,
            seconds,
        })
    }

    /// Whether no entity the firing reached has a think pending (FIRE-3.2).
    fn fire_is_quiet(&self) -> bool {
        self.builtins.tracer.as_ref().is_none_or(|tracer| {
            tracer
                .affected
                .iter()
                .all(|&n| !self.in_use(n) || self.field(n, "nextthink").as_f32() <= 0.0)
        })
    }

    /// Moves what the tracer recorded since the last call into `events`, its strings and
    /// classnames read, then the fields written on the entities the firing reached
    /// (FIRE-2.7).
    fn take_trace(
        &mut self,
        progs: &Progs<'_>,
        classnames: &mut BTreeMap<usize, String>,
        events: &mut Vec<TraceEvent>,
    ) {
        let Some(tracer) = &mut self.builtins.tracer else {
            return;
        };
        let raw = core::mem::take(&mut tracer.raw);
        let mut written = core::mem::take(&mut tracer.events);
        written.extend(self.machine.edicts_mut().take_events());
        let text = |world: &Self, value: Slot| {
            String::from_utf8_lossy(&world.string_value(progs, value).unwrap_or_default())
                .into_owned()
        };
        for record in raw {
            // An entity's classname now, or the one it had when it is gone.
            let mut classname = |world: &Self, entity: usize| {
                if world.in_use(entity) {
                    let name = world.classname(progs, entity);
                    classnames.insert(entity, name.clone());
                    name
                } else {
                    classnames.get(&entity).cloned().unwrap_or_default()
                }
            };
            events.push(match record {
                Raw::Ran {
                    time,
                    callback,
                    entity,
                    function,
                } => TraceEvent::Ran {
                    time,
                    callback,
                    entity,
                    classname: classname(self, entity),
                    function: usize::try_from(function)
                        .ok()
                        .and_then(|index| progs.functions().get(index))
                        .and_then(|function| progs.string(function.s_name))
                        .map_or_default(|name| String::from_utf8_lossy(name).into_owned()),
                },
                Raw::Looked {
                    time,
                    by,
                    name,
                    found,
                } => TraceEvent::Looked {
                    time,
                    by,
                    name: text(self, name),
                    found,
                },
                Raw::Unanswered { time, by, entity } => TraceEvent::Unanswered {
                    time,
                    by,
                    entity,
                    classname: classname(self, entity),
                },
                Raw::Removed { time, entity } => TraceEvent::Removed {
                    time,
                    entity,
                    classname: classnames.remove(&entity).unwrap_or_default(),
                },
                Raw::Spawned { time, entity } => TraceEvent::Spawned {
                    time,
                    entity,
                    classname: classname(self, entity),
                },
                Raw::Print { time, parts } => TraceEvent::Print {
                    time,
                    text: parts.into_iter().map(|part| text(self, part)).collect(),
                },
                Raw::Sound {
                    time,
                    entity,
                    sample,
                } => TraceEvent::Sound {
                    time,
                    entity,
                    sample: text(self, sample),
                },
            });
        }
        let time = double_to_float(self.time());
        let affected = self
            .builtins
            .tracer
            .as_ref()
            .map_or_default(|tracer| tracer.affected.clone());
        for change in self.changes_of(progs, written) {
            if let EdictChange::Changed { edict, fields } = change
                && affected.contains(&edict)
            {
                events.push(TraceEvent::Changed {
                    time,
                    entity: edict,
                    fields,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests;
