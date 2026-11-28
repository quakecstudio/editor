// SPDX-License-Identifier: GPL-3.0-or-later

//! The bridge's debugging commands on the game (BRIDGE-4.9, WORLD-9): breakpoints by file and
//! line, a frame stopped at one, `step` and `resume` replaying the frame from its snapshot, the
//! stopped call stack and watch expressions.

use qc_progs::Progs;
use qc_vm::{ErrorKind, StackEntry};
use qs_bridge::{ErrorCode, RpcError};
use qs_sim::debug::BreakpointEdit;
use qs_sim::debug::{Debugger, StepKind, Stop, locate, resolve};
use qs_sim::spawn::LoadError;
use qs_sim::{World, WorldError};
use serde_json::{Map, Value, json};

use super::{FrameEnd, Game, NextLevel, Reason, State};
use crate::values::to_json;

/// The host's id for a breakpoint it took, which `breakpoint/set` answers (BRIDGE-4.9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BreakpointId(u64);

impl BreakpointId {
    /// The breakpoint numbered `id`.
    pub(crate) const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Its number.
    pub(crate) const fn get(self) -> u64 {
        self.0
    }
}

/// A breakpoint the client set: its id and the statement it stops before.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Breakpoint {
    pub(crate) id: BreakpointId,
    pub(crate) statement: i32,
    /// The file and line it was set at, resolved again when the progs is reloaded.
    pub(crate) file: String,
    pub(crate) line: i32,
}

/// The world before the frame the debugger may stop, and that frame's time (WORLD-9.2).
#[derive(Clone)]
pub(crate) struct Snapshot {
    pub(crate) world: World,
    pub(crate) frametime: f64,
}

/// Where a frame stopped and its call stack, innermost first.
#[derive(Clone, Debug)]
pub(crate) struct Stopped {
    pub(crate) stop: Stop,
    pub(crate) stack: Vec<StackEntry>,
}

/// What a `step` asks for (BRIDGE-4.9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// A statement, into, over or out.
    Code(StepKind),
    /// The rest of the server frame.
    Frame,
}

impl Step {
    pub(super) fn from_name(name: &str) -> Option<Self> {
        match name {
            "statement" => Some(Self::Code(StepKind::Statement)),
            "in" => Some(Self::Code(StepKind::In)),
            "over" => Some(Self::Code(StepKind::Over)),
            "out" => Some(Self::Code(StepKind::Out)),
            "frame" => Some(Self::Frame),
            _ => None,
        }
    }
}

impl Game {
    /// The location of `statement` of `function` as the bridge writes it (BRIDGE-3.4).
    pub(super) fn location_json(&self, progs: &Progs<'_>, function: i32, statement: i32) -> Value {
        let location = locate(progs, &self.lines, function, statement);
        let mut out = Map::new();
        out.insert("function".to_owned(), json!(location.function));
        if let (Some((file, line)), Ok(statement)) =
            (location.source, u32::try_from(location.statement))
            && line > 0
            && !file.is_empty()
        {
            out.insert("file".to_owned(), json!(file));
            out.insert("line".to_owned(), json!(line));
            out.insert("statement".to_owned(), json!(statement));
        }
        Value::Object(out)
    }

    /// A stack as `inspect` and `qc/error` list it, innermost first (BRIDGE-5.2).
    pub(super) fn stack_json(&self, progs: &Progs<'_>, stack: &[StackEntry]) -> Vec<Value> {
        // The engine's own entry frame, function 0, has no QuakeC to show.
        stack
            .iter()
            .filter(|entry| entry.function > 0)
            .enumerate()
            .map(|(index, entry)| {
                let mut frame = self.location_json(progs, entry.function, entry.statement);
                if let Value::Object(map) = &mut frame {
                    map.insert("index".to_owned(), json!(index));
                }
                frame
            })
            .collect()
    }

    /// `breakpoint/set` (BRIDGE-4.9, WORLD-9.4).
    pub(super) fn breakpoint_set(
        &mut self,
        progs: &Progs<'_>,
        params: &Value,
    ) -> Result<Value, RpcError> {
        let file = params
            .get("file")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let line = params
            .get("line")
            .and_then(Value::as_i64)
            .and_then(|l| i32::try_from(l).ok())
            .unwrap_or(0);
        let statement = resolve(progs, &self.lines, file, line).ok_or_else(|| {
            RpcError::new(
                ErrorCode::NoSuchBreakpoint,
                format!("no statement is on line {line} of {file}"),
            )
        })?;
        let id = self.next_breakpoint;
        self.next_breakpoint = id.saturating_add(1);
        self.breakpoints.push(Breakpoint {
            id: BreakpointId::new(id),
            statement,
            file: file.to_owned(),
            line,
        });
        self.debugger()
            .set_breakpoint(statement, BreakpointEdit::Set);
        let function = function_of(progs, statement);
        Ok(json!({ "id": id, "location": self.location_json(progs, function, statement) }))
    }

    /// `breakpoint/clear` (BRIDGE-4.9).
    pub(super) fn breakpoint_clear(&mut self, params: &Value) -> Result<Value, RpcError> {
        let id = params.get("id").and_then(Value::as_u64);
        let at = self
            .breakpoints
            .iter()
            .position(|b| Some(b.id.get()) == id)
            .ok_or_else(|| RpcError::new(ErrorCode::NoSuchBreakpoint, "no such breakpoint"))?;
        let removed = self.breakpoints.remove(at);
        if !self
            .breakpoints
            .iter()
            .any(|b| b.statement == removed.statement)
        {
            self.debugger()
                .set_breakpoint(removed.statement, BreakpointEdit::Clear);
        }
        Ok(json!({}))
    }

    /// The world's debugger, made the first time it is needed.
    pub(super) fn debugger(&mut self) -> &mut Debugger {
        self.world.debugger_or_new(&self.lines)
    }

    /// Runs a frame of `frametime` from the world as it is, snapshotting it first when a
    /// debugger may stop it (WORLD-9.2).
    pub(super) fn run_frame(&mut self, progs: &Progs<'_>, frametime: f64) -> FrameEnd {
        // BOT-9.1: the bots' moves, which a replay of the frame keeps in its snapshot.
        if let Err(error) = self.drive_bots(progs, frametime) {
            self.fail(Some(progs), &error);
            return FrameEnd::Failed;
        }
        if self.world.debugger().is_some_and(Debugger::may_stop) {
            self.snapshot = Some(Snapshot {
                world: self.world.clone(),
                frametime,
            });
        }
        self.finish_frame_of(progs, frametime)
    }

    /// Replays the stopped frame from its snapshot with the debugger's new rule (WORLD-9.2), or
    /// the stopped load's spawning from the prepared world (WORLD-9.7).
    pub(super) fn replay(&mut self, progs: &Progs<'_>) -> FrameEnd {
        if self.loading.holds() {
            return self.replay_spawning(progs);
        }
        let Some(snapshot) = self.snapshot.clone() else {
            return FrameEnd::Ran;
        };
        let debugger = self.world.debugger().cloned();
        self.world = snapshot.world;
        self.world.set_debugger(debugger);
        self.finish_frame_of(progs, snapshot.frametime)
    }

    /// The debugger a level started from this one loads with: these breakpoints, from the
    /// start of the spawning (WORLD-9.7); none when there is no breakpoint to stop at.
    pub(crate) fn level_debugger(&self) -> Option<Debugger> {
        let mut debugger = self.world.debugger()?.clone();
        debugger.run_to_breakpoint(0);
        debugger.may_stop().then_some(debugger)
    }

    /// Holds this level's load, which a spawn function stopped, until going on spawns it to its
    /// end; the host then loads `level` again with no stop (WORLD-9.7).
    pub(crate) fn hold_load(&mut self, spawn: HeldSpawn, level: NextLevel) {
        self.loading = Loading::Held { spawn, level };
    }

    /// Pauses at the stop a held load's spawning made, once [`Game::carry_from`] has given the
    /// game the breakpoints that name it (WORLD-9.7).
    pub(super) fn report_held_stop(&mut self, progs: &Progs<'_>) {
        let stack = match &mut self.loading {
            Loading::Held { spawn, .. } => spawn.stack.take(),
            Loading::Done | Loading::Spawned(_) => None,
        };
        if let Some(stack) = stack {
            self.stopped_at(progs, stack);
        }
    }

    /// The level to load again once a held load's spawning has ended.
    pub(super) fn take_spawned(&mut self) -> Option<NextLevel> {
        match core::mem::take(&mut self.loading) {
            Loading::Spawned(level) => Some(level),
            other => {
                self.loading = other;
                None
            }
        }
    }

    /// Spawns the held load's entities again from its prepared world with the debugger's rule:
    /// paused at the next stop, or, once the spawning ends, the level asked for again.
    fn replay_spawning(&mut self, progs: &Progs<'_>) -> FrameEnd {
        let Loading::Held { spawn, .. } = &self.loading else {
            return FrameEnd::Ran;
        };
        let debugger = self.world.debugger().cloned();
        self.world = (*spawn.prepared).clone();
        self.world.set_debugger(debugger);
        match self.world.spawn_map(progs) {
            Err(WorldError::Spawn(LoadError::Run(error)))
                if error.kind == ErrorKind::Interrupted =>
            {
                drop(self.world.take_messages(qs_sim::ClientId::FIRST));
                self.stopped_at(progs, error.trace);
                FrameEnd::Stopped
            }
            Ok(()) => {
                self.halted = None;
                if let Loading::Held { level, .. } = core::mem::take(&mut self.loading) {
                    self.loading = Loading::Spawned(level);
                }
                FrameEnd::Ran
            }
            Err(error) => {
                self.fail(Some(progs), &error);
                FrameEnd::Failed
            }
        }
    }

    /// Runs the frame and answers for how it ended: on to the next frame's breakpoints, paused
    /// at a stop (BRIDGE-3.3), or failed with its `qc/error`.
    fn finish_frame_of(&mut self, progs: &Progs<'_>, frametime: f64) -> FrameEnd {
        let outcome = self.world.frame(progs, frametime);
        if let Err(WorldError::Frame(error)) = &outcome
            && error.kind == ErrorKind::Interrupted
        {
            // The half frame's messages never reach the client: the frame replays from its
            // snapshot, which writes them again (WORLD-9.2).
            drop(self.world.take_messages(qs_sim::ClientId::FIRST));
            self.stopped_at(progs, error.trace.clone());
            return FrameEnd::Stopped;
        }
        self.halted = None;
        self.failed = None;
        self.snapshot = None;
        if let Some(debugger) = self.world.debugger_mut() {
            debugger.run_to_breakpoint(0);
        }
        self.report_changes(progs);
        self.report_console();
        match outcome {
            Ok(()) => FrameEnd::Ran,
            Err(error) => {
                self.fail(Some(progs), &error);
                FrameEnd::Failed
            }
        }
    }

    /// The frame stopped: paused there, with `breakpoint/hit` when a breakpoint stopped it and
    /// `state/changed` with its reason (BRIDGE-3.1, BRIDGE-3.3).
    fn stopped_at(&mut self, progs: &Progs<'_>, stack: Vec<StackEntry>) {
        let Some(stop) = self.world.debugger().and_then(Debugger::stopped) else {
            return;
        };
        self.state = State::Paused;
        let location = self.location_json(progs, stop.function, stop.statement);
        let time = self.world.time();
        let breakpoint = stop
            .breakpoint
            .and_then(|statement| self.breakpoints.iter().find(|b| b.statement == statement));
        let reason = if let Some(breakpoint) = breakpoint {
            let me = self.world.evaluate(progs, None, "self");
            let mut params = Map::new();
            params.insert("breakpoint".to_owned(), json!(breakpoint.id.get()));
            params.insert("location".to_owned(), location.clone());
            if let Ok(qs_sim::QcValue::Entity(edict)) = me {
                params.insert("self".to_owned(), json!(edict));
            }
            params.insert("time".to_owned(), json!(time));
            self.events.push(("breakpoint/hit", Value::Object(params)));
            Reason::Breakpoint
        } else {
            Reason::Step
        };
        self.halted = Some(Stopped { stop, stack });
        self.state_changed(Some(reason), Some(location));
    }

    /// `step` (BRIDGE-4.9, WORLD-9.3): from a stop, a replay to where `step` goes; from between
    /// two frames, the next frame up to its first statement, or the whole frame.
    pub(super) fn debug_step(&mut self, progs: &Progs<'_>, step: Step, frametime: f64) {
        let stop = self.halted.as_ref().map(|s| s.stop);
        let end = match (step, stop) {
            (Step::Frame, Some(_)) => {
                self.debugger().finish();
                self.replay(progs)
            }
            (Step::Code(kind), Some(stop)) => {
                self.debugger().step_from(kind, &stop);
                self.replay(progs)
            }
            (Step::Frame, None) => self.run_frame(progs, frametime),
            (Step::Code(_), None) => {
                self.debugger().stop_at_first();
                self.run_frame(progs, frametime)
            }
        };
        // A stop has said where it paused; a frame that ran to its end pauses between two.
        if end == FrameEnd::Ran {
            self.state_changed(Some(Reason::Step), None);
        }
    }

    /// `resume` from a stop (BRIDGE-4.9): the frame replayed past it, to a breakpoint or its
    /// end. Returns whether the game runs again.
    pub(super) fn resume_from_stop(&mut self, progs: &Progs<'_>) -> bool {
        let Some(stopped) = self.halted.clone() else {
            return true;
        };
        self.debugger().run_to_breakpoint(stopped.stop.count);
        self.replay(progs) == FrameEnd::Ran
    }

    /// The stack the game is paused in: a stop's, or the run's a run error ended
    /// (BRIDGE-4.20); `None` between frames.
    fn paused_stack(&self) -> Option<&[StackEntry]> {
        match &self.halted {
            Some(stopped) => Some(&stopped.stack),
            None => self.failed.as_deref(),
        }
    }

    /// `inspect` with `stack` (BRIDGE-4.9, BRIDGE-4.20): the stopped or failed frame's, empty
    /// between frames.
    pub(super) fn stack_now(&self, progs: &Progs<'_>) -> Vec<Value> {
        self.paused_stack()
            .map_or_default(|stack| self.stack_json(progs, stack))
    }

    /// `evaluate` (BRIDGE-4.9, WORLD-9.5) in stack frame `frame`, 0 the innermost.
    pub(super) fn evaluate(&self, progs: &Progs<'_>, params: &Value) -> Result<Value, RpcError> {
        self.require_paused()?;
        let expression = params
            .get("expression")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let frame = params
            .get("frame")
            .and_then(Value::as_u64)
            .and_then(|f| usize::try_from(f).ok())
            .unwrap_or(0);
        let function = self
            .paused_stack()
            .and_then(|stack| stack.get(frame))
            .map(|entry| entry.function);
        let value = self
            .world
            .evaluate(progs, function, expression)
            .map_err(|e| RpcError::new(ErrorCode::BadValue, e.to_string()))?;
        Ok(json!({ "value": to_json(&value) }))
    }
}

/// The function whose body holds `statement`: the one with the greatest first statement at or
/// before it.
fn function_of(progs: &Progs<'_>, statement: i32) -> i32 {
    let mut best: (i32, i32) = (0, 0);
    for (index, function) in progs.functions().iter().enumerate() {
        let first = function.first_statement;
        if first > 0 && first <= statement && first > best.1 {
            best = (i32::try_from(index).unwrap_or(0), first);
        }
    }
    best.0
}

/// A level's load a spawn function stopped (WORLD-9.7): the world before its spawning, which
/// going on spawns again from, and the stopped run's stack until the game reports the stop.
pub(crate) struct HeldSpawn {
    prepared: Box<World>,
    stack: Option<Vec<StackEntry>>,
}

impl HeldSpawn {
    /// Spawns `world`'s map watched by `debugger` (WORLD-9.6). `None` once the spawning has
    /// ended, the debugger taken off so the level settles and the player enters as with none;
    /// the held load when a spawn function stopped it.
    ///
    /// # Errors
    ///
    /// The spawning failed other than by a stop.
    pub(crate) fn spawn(
        world: &mut World,
        progs: &Progs<'_>,
        debugger: Debugger,
    ) -> Result<Option<Self>, WorldError> {
        let prepared = Box::new(world.clone());
        world.set_debugger(Some(debugger));
        match world.spawn_map(progs) {
            Ok(()) => {
                world.set_debugger(None);
                Ok(None)
            }
            Err(WorldError::Spawn(LoadError::Run(error)))
                if error.kind == ErrorKind::Interrupted =>
            {
                drop(world.take_messages(qs_sim::ClientId::FIRST));
                Ok(Some(Self {
                    prepared,
                    stack: Some(error.trace),
                }))
            }
            Err(error) => Err(error),
        }
    }
}

/// Where a level's load is (WORLD-9.7).
#[derive(Default)]
pub(super) enum Loading {
    /// Loaded: the level runs.
    #[default]
    Done,
    /// A spawn function stopped the load, which holds the game paused in it.
    Held {
        /// The prepared world and the stop's stack.
        spawn: HeldSpawn,
        /// The level, loaded again once the spawning ends.
        level: NextLevel,
    },
    /// The held load's spawning has ended: the host loads the level again.
    Spawned(NextLevel),
}

impl Loading {
    /// Whether the load holds the level, which runs no frame until it is loaded.
    pub(super) const fn holds(&self) -> bool {
        !matches!(self, Self::Done)
    }
}
