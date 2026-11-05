// SPDX-License-Identifier: GPL-3.0-or-later

//! The game as the bridge drives it (BRIDGE-3, BRIDGE-4): the server's world and its progs,
//! paused or running, the edits held while paused, and the events each command and frame
//! raises.

use qc_progs::Progs;
use qs_bridge::{Capabilities, ErrorCode, Host, HostInfo, RpcError};
use qs_sim::{EdictChange, EditError, World, WorldError, command_words};
use serde_json::{Map, Value, json};

use crate::values::{from_json, named_json, to_json};

/// The commands the game accepts; `pick` and `screenshot` come with the window.
const COMMANDS: [&str; 9] = [
    "console",
    "pause",
    "resume",
    "step",
    "field/set",
    "entity/spawn",
    "entity/remove",
    "entity/edit",
    "player/spawn_at",
];

/// Whether the server clock runs (BRIDGE-4.1, "pause, resume").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Running,
    Paused,
}

/// Why the game paused, as `state/changed` says (BRIDGE-3.3).
#[derive(Clone, Copy, Debug)]
enum Reason {
    Pause,
    Step,
    Error,
}

impl Reason {
    const fn name(self) -> &'static str {
        match self {
            Self::Pause => "pause",
            Self::Step => "step",
            Self::Error => "error",
        }
    }
}

/// An edit asked for while paused, applied in order when the game resumes (BRIDGE-4.4).
#[derive(Clone, Debug)]
enum Held {
    Field {
        edict: usize,
        field: String,
        value: qs_sim::QcValue,
    },
    Keys {
        edict: usize,
        pairs: Vec<(String, String)>,
    },
}

/// When an edit applies, as its result says (BRIDGE-4.4).
#[derive(Clone, Copy, Debug)]
enum Applied {
    Now,
    OnResume,
}

impl Applied {
    fn to_json(self) -> Value {
        json!({ "applied": match self {
            Self::Now => "now",
            Self::OnResume => "on_resume",
        } })
    }
}

/// An event for the bridge to send: its method and parameters (BRIDGE-3.1).
pub(crate) type Event = (&'static str, Value);

/// The game: its world, the progs it runs, and what the bridge has asked of it.
pub(crate) struct Game {
    world: World,
    progs: Vec<u8>,
    state: State,
    held: Vec<Held>,
    events: Vec<Event>,
    /// Whether the host was asked to stop (BRIDGE-2.5).
    stopped: bool,
}

impl Game {
    /// The game of `world`, loaded from `progs`, running.
    pub(crate) fn new(mut world: World, progs: Vec<u8>) -> Self {
        world.track_changes(true);
        Self {
            world,
            progs,
            state: State::Running,
            held: Vec::new(),
            events: Vec::new(),
            stopped: false,
        }
    }

    /// The events raised since the last call, oldest first.
    pub(crate) fn take_events(&mut self) -> Vec<Event> {
        core::mem::take(&mut self.events)
    }

    /// Whether the client asked the host to stop.
    pub(crate) const fn stopped(&self) -> bool {
        self.stopped
    }

    /// The world, for drawing.
    pub(crate) const fn world(&self) -> &World {
        &self.world
    }

    /// The world, for the player's commands and the level changes.
    pub(crate) const fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// The progs the world runs.
    #[cfg(test)]
    pub(crate) fn progs(&self) -> &[u8] {
        &self.progs
    }

    /// `progs/loaded` for the progs at `path`, and `map/loaded` (BRIDGE-3.1).
    pub(crate) fn announce(&mut self, path: &str, map: &str) {
        let time = self.world.time();
        let Ok(header) = Progs::read(&self.progs).map(|progs| *progs.header()) else {
            return;
        };
        let progs_loaded = json!({
            "path": path,
            "crc": header.crc,
            "version": header.version,
            "functions": header.numfunctions,
            "globals": header.numglobals,
            "fields": header.numfielddefs,
            "lno": false,
            "time": time,
        });
        let entities = (0..self.world.num_edicts())
            .filter(|&n| self.world.in_use(n))
            .count();
        self.events.push(("progs/loaded", progs_loaded));
        self.events.push((
            "map/loaded",
            json!({ "map": map, "entities": entities, "time": time }),
        ));
    }

    /// Runs one server frame of `frametime` seconds unless paused, then raises the edicts'
    /// events (BRIDGE-3.2); a frame that fails pauses the game with a `qc/error`
    /// (BRIDGE-3.3). Returns whether a frame ran.
    pub(crate) fn frame(&mut self, frametime: f64) -> bool {
        if self.state == State::Paused {
            return false;
        }
        let ran = self.with_progs(|game, progs| game.run_frame(progs, frametime));
        if let Err(error) = ran {
            self.fail(&WorldError::Progs(error.message));
        }
        true
    }

    /// Runs `f` with the progs read from its bytes; the error says why they did not read.
    fn with_progs<T>(&mut self, f: impl FnOnce(&mut Self, &Progs<'_>) -> T) -> Result<T, RpcError> {
        let bytes = core::mem::take(&mut self.progs);
        let outcome = match Progs::read(&bytes) {
            Ok(progs) => Ok(f(self, &progs)),
            Err(error) => Err(RpcError::new(ErrorCode::InternalError, error.to_string())),
        };
        self.progs = bytes;
        outcome
    }

    fn run_frame(&mut self, progs: &Progs<'_>, frametime: f64) {
        let outcome = self.world.frame(progs, frametime);
        self.report_changes(progs);
        if let Err(error) = outcome {
            self.fail(&error);
        }
    }

    /// The `qc/error` of a failed frame, then the pause it causes (BRIDGE-3.3).
    fn fail(&mut self, error: &WorldError) {
        let time = self.world.time();
        let mut params = Map::new();
        params.insert("message".to_owned(), json!(error.to_string()));
        params.insert("time".to_owned(), json!(time));
        let mut location = None;
        if let WorldError::Frame(run) = error {
            let stack: Vec<Value> = run
                .trace
                .iter()
                .enumerate()
                .map(|(index, entry)| json!({ "index": index, "function": entry.name }))
                .collect();
            location = run
                .trace
                .first()
                .map(|innermost| json!({ "function": innermost.name }));
            if let Some(location) = &location {
                params.insert("location".to_owned(), location.clone());
            }
            params.insert("stack".to_owned(), Value::Array(stack));
        }
        tracing::warn!(%error, "the game stopped");
        self.events.push(("qc/error", Value::Object(params)));
        self.state = State::Paused;
        self.state_changed(Some(Reason::Error), location);
    }

    /// `edict/spawned`, `edict/removed` and `edict/changed` for what happened since the last
    /// report (BRIDGE-3.1, BRIDGE-3.2).
    fn report_changes(&mut self, progs: &Progs<'_>) {
        let time = self.world.time();
        for change in self.world.take_changes(progs) {
            self.events.push(match change {
                EdictChange::Spawned { edict, classname } => (
                    "edict/spawned",
                    json!({ "edict": edict, "classname": classname, "time": time }),
                ),
                EdictChange::Removed(edict) => {
                    ("edict/removed", json!({ "edict": edict, "time": time }))
                }
                EdictChange::Changed { edict, fields } => (
                    "edict/changed",
                    json!({ "edict": edict, "fields": named_json(&fields), "time": time }),
                ),
            });
        }
    }

    /// `state/changed` to the state now, with why and where (BRIDGE-3.3).
    fn state_changed(&mut self, reason: Option<Reason>, location: Option<Value>) {
        let mut params = Map::new();
        let state = match self.state {
            State::Running => "running",
            State::Paused => "paused",
        };
        params.insert("state".to_owned(), json!(state));
        params.insert("time".to_owned(), json!(self.world.time()));
        if let Some(reason) = reason {
            params.insert("reason".to_owned(), json!(reason.name()));
        }
        if let Some(location) = location {
            params.insert("location".to_owned(), location);
        }
        self.events.push(("state/changed", Value::Object(params)));
    }

    fn time(&self) -> Value {
        json!({ "time": self.world.time() })
    }

    fn require_paused(&self) -> Result<(), RpcError> {
        match self.state {
            State::Paused => Ok(()),
            State::Running => Err(RpcError::new(
                ErrorCode::NotPaused,
                "the game is running; pause it first",
            )),
        }
    }

    fn require_in_use(&self, edict: usize) -> Result<(), RpcError> {
        if self.world.in_use(edict) {
            Ok(())
        } else {
            Err(edit_error(&EditError::NoSuchEdict(edict)))
        }
    }

    fn inspect(&self, progs: &Progs<'_>, params: &Value) -> Result<Value, RpcError> {
        let time = self.world.time();
        match params.get("what").and_then(Value::as_str) {
            Some("edict") => {
                let edict = edict_param(params)?;
                let fields = self
                    .world
                    .edict_fields(progs, edict)
                    .ok_or_else(|| edit_error(&EditError::NoSuchEdict(edict)))?;
                Ok(json!({
                    "edict": edict,
                    "classname": self.world.classname(progs, edict),
                    "fields": named_json(&fields),
                    "time": time,
                }))
            }
            Some("globals") => Ok(json!({
                "globals": named_json(&self.world.named_globals(progs)),
                "time": time,
            })),
            // Paused between two frames, no QuakeC is running: the stack is empty.
            Some("stack") => {
                self.require_paused()?;
                Ok(json!({ "stack": [], "time": time }))
            }
            Some("entities") => {
                let entities: Vec<Value> = (0..self.world.num_edicts())
                    .filter(|&n| self.world.in_use(n))
                    .map(|n| self.entity_summary(progs, n))
                    .collect();
                Ok(json!({ "entities": entities, "time": time }))
            }
            _ => Err(RpcError::new(
                ErrorCode::InvalidParams,
                "nothing to inspect",
            )),
        }
    }

    /// An edict in use as `inspect` lists it: number, classname, model and origin.
    fn entity_summary(&self, progs: &Progs<'_>, n: usize) -> Value {
        let fields = self.world.edict_fields(progs, n).unwrap_or_default();
        let mut entity = Map::new();
        entity.insert("edict".to_owned(), json!(n));
        entity.insert(
            "classname".to_owned(),
            json!(self.world.classname(progs, n)),
        );
        for named in fields {
            if matches!(named.name.as_str(), "model" | "origin")
                && let Some(value) = to_json(&named.value).get("value")
            {
                entity.insert(named.name, value.clone());
            }
        }
        Value::Object(entity)
    }

    fn field_set(&mut self, progs: &Progs<'_>, params: &Value) -> Result<Value, RpcError> {
        let edict = edict_param(params)?;
        let field = params
            .get("field")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let value = params
            .get("value")
            .and_then(from_json)
            .ok_or_else(|| RpcError::new(ErrorCode::BadValue, "the value does not read"))?;
        self.world
            .check_field_value(progs, edict, &field, &value)
            .map_err(|e| edit_error(&e))?;
        if self.state == State::Paused {
            self.held.push(Held::Field {
                edict,
                field,
                value,
            });
            return Ok(Applied::OnResume.to_json());
        }
        self.world
            .set_field_value(progs, edict, &field, &value)
            .map_err(|e| edit_error(&e))?;
        self.report_changes(progs);
        Ok(Applied::Now.to_json())
    }

    fn entity_edit(&mut self, progs: &Progs<'_>, params: &Value) -> Result<Value, RpcError> {
        let edict = edict_param(params)?;
        self.require_in_use(edict)?;
        let pairs = keyvalues(params);
        if self.state == State::Paused {
            self.held.push(Held::Keys { edict, pairs });
            return Ok(Applied::OnResume.to_json());
        }
        self.world
            .edit_entity(progs, edict, &pairs)
            .map_err(|e| edit_error(&e))?;
        self.report_changes(progs);
        Ok(Applied::Now.to_json())
    }

    /// Applies the edits held while paused, in order (BRIDGE-4.4); one that no longer applies,
    /// its edict freed since, is dropped with a warning.
    fn apply_held(&mut self, progs: &Progs<'_>) {
        for held in core::mem::take(&mut self.held) {
            let outcome = match &held {
                Held::Field {
                    edict,
                    field,
                    value,
                } => self.world.set_field_value(progs, *edict, field, value),
                Held::Keys { edict, pairs } => self.world.edit_entity(progs, *edict, pairs),
            };
            if let Err(error) = outcome {
                tracing::warn!(%error, ?held, "a held edit no longer applies");
            }
        }
        self.report_changes(progs);
    }

    fn step(&mut self, progs: &Progs<'_>, params: &Value) -> Result<Value, RpcError> {
        self.require_paused()?;
        match params.get("kind").and_then(Value::as_str) {
            Some("frame") => {
                self.run_frame(progs, STEP_FRAME);
                if self.state == State::Paused {
                    self.state_changed(Some(Reason::Step), None);
                }
                Ok(self.time())
            }
            // Statement stepping comes with the debugger (P3-04).
            _ => Err(RpcError::new(
                ErrorCode::Unsupported,
                "only a whole frame can be stepped",
            )),
        }
    }

    fn console(&mut self, params: &Value) -> Value {
        let text = params
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or_default();
        for words in command_words(text.as_bytes()) {
            let name = words
                .first()
                .map(|w| String::from_utf8_lossy(w).into_owned());
            // `Cmd_ExecuteString`: `Con_Printf ("Unknown command \"%s\"\n", Cmd_Argv(0))`.
            self.events.push((
                "console/line",
                json!({
                    "text": format!("Unknown command \"{}\"", name.unwrap_or_default()),
                    "level": "print",
                    "time": self.world.time(),
                }),
            ));
        }
        json!({})
    }
}

/// The frame a `step` of kind `frame` runs: `Host_FilterTime`'s shortest, 1/72 of a second.
const STEP_FRAME: f64 = 1.0 / 72.0;

impl Host for Game {
    fn info(&self) -> HostInfo {
        HostInfo {
            name: "preview-host".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            engine: "id".to_owned(),
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::of(COMMANDS)
    }

    fn call(&mut self, method: &str, params: &Value) -> Result<Value, RpcError> {
        self.with_progs(|game, progs| game.dispatch(progs, method, params))?
    }

    fn shutdown(&mut self) {
        self.stopped = true;
    }
}

impl Game {
    fn dispatch(
        &mut self,
        progs: &Progs<'_>,
        method: &str,
        params: &Value,
    ) -> Result<Value, RpcError> {
        match method {
            "inspect" => self.inspect(progs, params),
            "pause" => {
                if self.state == State::Running {
                    self.state = State::Paused;
                    self.state_changed(Some(Reason::Pause), None);
                }
                Ok(self.time())
            }
            "resume" => {
                if self.state == State::Paused {
                    self.apply_held(progs);
                    self.state = State::Running;
                    self.state_changed(None, None);
                }
                Ok(self.time())
            }
            "step" => self.step(progs, params),
            "field/set" => self.field_set(progs, params),
            "entity/spawn" => {
                let mut pairs = vec![(
                    "classname".to_owned(),
                    params
                        .get("classname")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                )];
                pairs.extend(keyvalues(params));
                let edict = self
                    .world
                    .spawn_entity(progs, &pairs)
                    .map_err(|e| edit_error(&e))?;
                self.report_changes(progs);
                Ok(json!({ "edict": edict }))
            }
            "entity/remove" => {
                let edict = edict_param(params)?;
                self.world
                    .remove_entity(progs, edict)
                    .map_err(|e| edit_error(&e))?;
                self.report_changes(progs);
                Ok(json!({}))
            }
            "entity/edit" => self.entity_edit(progs, params),
            "player/spawn_at" => {
                let origin = vector_param(params, "origin");
                let angles = vector_param(params, "angles");
                self.world
                    .move_player(progs, origin, angles)
                    .map_err(|e| edit_error(&e))?;
                self.report_changes(progs);
                Ok(json!({ "edict": qs_sim::CLIENT_EDICT }))
            }
            "console" => Ok(self.console(params)),
            _ => Err(RpcError::new(
                ErrorCode::MethodNotFound,
                format!("{method} is not a command"),
            )),
        }
    }
}

/// The error an edit answers with (BRIDGE-7.1).
fn edit_error(error: &EditError) -> RpcError {
    let code = match error {
        EditError::NoSuchEdict(_) => ErrorCode::NoSuchEdict,
        EditError::NoSuchField(_) | EditError::BadValue(_) => ErrorCode::BadValue,
        EditError::Kept(_) | EditError::Spawn(_) => ErrorCode::InvalidParams,
    };
    RpcError::new(code, error.to_string())
}

/// The `edict` parameter, which the schema has checked is a number (BRIDGE-4.2).
fn edict_param(params: &Value) -> Result<usize, RpcError> {
    params
        .get("edict")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| RpcError::new(ErrorCode::InvalidParams, "no edict"))
}

/// The vector parameter `name`, zero when it is absent (BRIDGE-5.2).
fn vector_param(params: &Value, name: &str) -> [f32; 3] {
    let value =
        json!({ "type": "vector", "value": params.get(name).cloned().unwrap_or(json!([0, 0, 0])) });
    match from_json(&value) {
        Some(qs_sim::QcValue::Vector(v)) => v,
        _ => [0.0; 3],
    }
}

/// The `keyvalues` parameter as the pairs a map entity holds (BRIDGE-5.2).
fn keyvalues(params: &Value) -> Vec<(String, String)> {
    params
        .get("keyvalues")
        .and_then(Value::as_array)
        .map(|pairs| {
            pairs
                .iter()
                .filter_map(|pair| {
                    Some((
                        pair.get("key")?.as_str()?.to_owned(),
                        pair.get("value")?.as_str()?.to_owned(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
