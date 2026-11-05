// SPDX-License-Identifier: GPL-3.0-or-later

//! The game as the bridge drives it (BRIDGE-3, BRIDGE-4): the server's world and its progs,
//! paused or running, the edits held while paused, and the events each command and frame
//! raises.

use qc_progs::Progs;
use qs_bridge::{Answer, Capabilities, ErrorCode, Host, HostInfo, Id, RpcError};
use qs_render::Camera;
use qs_sim::{EdictChange, EditError, World, WorldError, command_words};
use serde_json::{Map, Value, json};

use crate::screenshot::{Frame, encode};
use crate::values::{from_json, named_json, to_json};

/// The commands the game accepts.
const COMMANDS: [&str; 11] = [
    "console",
    "pause",
    "resume",
    "step",
    "field/set",
    "entity/spawn",
    "entity/remove",
    "entity/edit",
    "player/spawn_at",
    "pick",
    "screenshot",
];

/// How far `pick`'s ray reaches, in map units.
const PICK_REACH: f32 = 8192.0;

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

/// Whether anyone hears the game's events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reporting {
    /// The IDE is connected over the bridge: every event is raised (BRIDGE-3.1).
    Bridge,
    /// The game is played on its own: the edicts' changes are not recorded.
    Off,
}

/// How a server frame ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrameEnd {
    Ran,
    /// It stopped with a run error, which paused the game (BRIDGE-3.3).
    Failed,
}

/// An event for the bridge to send: its method and parameters (BRIDGE-3.1).
pub(crate) type Event = (&'static str, Value);

/// Where the window's view looks from, for `pick` (BRIDGE-4.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Eye {
    /// The eye's origin.
    pub(crate) origin: [f32; 3],
    /// Its pitch, yaw and roll.
    pub(crate) angles: [f32; 3],
    /// The field of view across, in degrees.
    pub(crate) fov_x: f32,
    /// The view's width per unit of height.
    pub(crate) aspect: f32,
}

/// A `screenshot` waiting for the next frame drawn, and the size it asked for.
#[derive(Clone, Debug)]
struct Shot {
    id: Id,
    size: Option<(u32, u32)>,
}

/// A response the game sends after the command's request was answered later.
pub(crate) type Response = (Id, Result<Value, RpcError>);

/// The game: its world, the progs it runs, and what the bridge has asked of it.
pub(crate) struct Game {
    world: World,
    progs: Vec<u8>,
    state: State,
    held: Vec<Held>,
    events: Vec<Event>,
    /// Where the window last looked from, `None` before it has drawn.
    eye: Option<Eye>,
    /// The screenshots asked for since the last frame drawn.
    shots: Vec<Shot>,
    /// The responses of commands answered later, to send.
    responses: Vec<Response>,
    /// Whether the host was asked to stop (BRIDGE-2.5).
    stopped: bool,
}

impl Game {
    /// The game of `world`, loaded from `progs`, running.
    pub(crate) fn new(mut world: World, progs: Vec<u8>, reporting: Reporting) -> Self {
        // The edicts' events cost a record of every field write; only a client reads them.
        world.track_changes(reporting == Reporting::Bridge);
        Self {
            world,
            progs,
            state: State::Running,
            held: Vec::new(),
            events: Vec::new(),
            eye: None,
            shots: Vec::new(),
            responses: Vec::new(),
            stopped: false,
        }
    }

    /// The events raised since the last call, oldest first.
    pub(crate) fn take_events(&mut self) -> Vec<Event> {
        core::mem::take(&mut self.events)
    }

    /// The responses of commands answered later since the last call.
    pub(crate) fn take_responses(&mut self) -> Vec<Response> {
        core::mem::take(&mut self.responses)
    }

    /// Where the window's view looks from now.
    pub(crate) const fn set_eye(&mut self, eye: Eye) {
        self.eye = Some(eye);
    }

    /// Whether a `screenshot` waits for the next frame drawn.
    pub(crate) const fn wants_screenshot(&self) -> bool {
        !self.shots.is_empty()
    }

    /// Answers every waiting `screenshot` with `frame`, the frame just drawn.
    pub(crate) fn screenshot_taken(&mut self, frame: &Frame) {
        let time = self.world.time();
        for shot in core::mem::take(&mut self.shots) {
            let copy = Frame {
                width: frame.width,
                height: frame.height,
                rgba: frame.rgba.clone(),
            };
            let outcome = encode(copy, shot.size)
                .map(|(png, width, height)| {
                    json!({ "png": png, "width": width, "height": height, "time": time })
                })
                .map_err(|why| RpcError::new(ErrorCode::InternalError, why));
            self.responses.push((shot.id, outcome));
        }
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

    fn run_frame(&mut self, progs: &Progs<'_>, frametime: f64) -> FrameEnd {
        let outcome = self.world.frame(progs, frametime);
        self.report_changes(progs);
        match outcome {
            Ok(()) => FrameEnd::Ran,
            Err(error) => {
                self.fail(&error);
                FrameEnd::Failed
            }
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
                // A frame that fails has said why it paused (BRIDGE-3.3).
                if self.run_frame(progs, STEP_FRAME) == FrameEnd::Ran {
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

    fn call(&mut self, id: &Id, method: &str, params: &Value) -> Answer {
        if method == "screenshot" {
            let size = |name: &str| {
                params
                    .get(name)
                    .and_then(Value::as_u64)
                    .and_then(|n| u32::try_from(n).ok())
            };
            self.shots.push(Shot {
                id: id.clone(),
                size: size("width").zip(size("height")),
            });
            return Answer::Later;
        }
        Answer::Now(
            self.with_progs(|game, progs| game.dispatch(progs, method, params))
                .and_then(|outcome| outcome),
        )
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
            "pick" => self.pick(progs, params),
            _ => Err(RpcError::new(
                ErrorCode::MethodNotFound,
                format!("{method} is not a command"),
            )),
        }
    }
}

impl Game {
    /// `pick`: the edict under the point `x`, `y` of the window, as fractions of its width
    /// and height from the top left, and where the ray from the eye stops (BRIDGE-4.1).
    fn pick(&mut self, progs: &Progs<'_>, params: &Value) -> Result<Value, RpcError> {
        let eye = self
            .eye
            .ok_or_else(|| RpcError::new(ErrorCode::InternalError, "nothing is drawn yet"))?;
        let fraction = |name: &str| {
            params
                .get(name)
                .and_then(Value::as_f64)
                .map_or(0.5, f64_to_f32)
        };
        let camera = Camera {
            origin: eye.origin,
            pitch: eye.angles[0],
            yaw: eye.angles[1],
            fov_x: eye.fov_x,
        };
        // Clip space runs from -1 at the left and bottom to 1 at the right and top.
        let at = [fraction("x") * 2.0 - 1.0, 1.0 - fraction("y") * 2.0];
        let (origin, direction) = camera.ray(eye.aspect, at);
        let mut end = origin;
        for (point, along) in end.iter_mut().zip(direction) {
            *point += along * PICK_REACH;
        }
        let hit = self
            .world
            .trace_line(progs, origin, end)
            .map_err(|e| RpcError::new(ErrorCode::InternalError, e.to_string()))?;
        let mut result = Map::new();
        if let Some(edict) = hit.edict {
            result.insert("edict".to_owned(), json!(edict));
        }
        if let Some(point) = hit.point {
            result.insert("point".to_owned(), json!(point));
        }
        Ok(Value::Object(result))
    }
}

/// A JSON number as a float, rounded as C rounds a double to a float.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a fraction of the window, exact enough in f32"
)]
const fn f64_to_f32(value: f64) -> f32 {
    value as f32
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
