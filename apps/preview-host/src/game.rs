// SPDX-License-Identifier: GPL-3.0-or-later

//! The game as the bridge drives it (BRIDGE-3, BRIDGE-4): the server's world and its progs,
//! paused or running, the edits held while paused, and the events each command and frame
//! raises.

use qc_progs::Progs;
use qs_bridge::{Answer, Capabilities, ErrorCode, Host, HostInfo, Id, RpcError};
use qs_render::Camera;
use qs_sim::{EdictChange, EditError, HostCommand, HostState, SpawnParms, World, WorldError};
use serde_json::{Map, Value, json};

use crate::screenshot::{Frame, encode};
use crate::values::{from_json, named_json, to_json};

mod debug;
mod reload;

use debug::{Breakpoint, Snapshot, Step, Stopped};

/// The commands the game accepts.
const COMMANDS: [&str; 16] = [
    "reload",
    "breakpoint/set",
    "breakpoint/clear",
    "evaluate",
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
    "window/place",
];

/// How far `pick`'s ray reaches, in map units.
const PICK_REACH: f32 = 8192.0;

/// Whether the host draws frames, and so can answer `screenshot`, and whether it can place its
/// window, and so can answer `window/place`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Screen {
    /// The game's window, which the host moves and sizes when asked.
    Window,
    /// The game's window on a windowing system that lets a client neither move nor raise its
    /// window (Wayland): `window/place` is not offered (BRIDGE-4.10).
    FixedWindow,
    /// `--headless` with a GPU device: a frame is drawn offscreen when `screenshot` asks.
    Offscreen,
    /// `--headless` with no GPU device: nothing is drawn, and `screenshot` is not offered.
    Headless,
}

impl Screen {
    /// Whether it can draw a frame for `screenshot`.
    const fn draws(self) -> bool {
        match self {
            Self::Window | Self::FixedWindow | Self::Offscreen => true,
            Self::Headless => false,
        }
    }

    /// Whether it has a window it can move, size and raise.
    const fn places(self) -> bool {
        match self {
            Self::Window => true,
            Self::FixedWindow | Self::Offscreen | Self::Headless => false,
        }
    }

    /// Whether it offers `command`.
    fn offers(self, command: &str) -> bool {
        match command {
            "screenshot" => self.draws(),
            "window/place" => self.places(),
            _ => true,
        }
    }
}

/// Where `window/place` puts the window (BRIDGE-4.10): its outer frame in the desktop's
/// logical points, and whether it stays above other windows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Placement {
    /// The frame's left edge.
    pub(crate) x: f32,
    /// Its top edge.
    pub(crate) y: f32,
    /// Its width, at least 1.
    pub(crate) width: f32,
    /// Its height, at least 1.
    pub(crate) height: f32,
    /// Whether it stays above other windows.
    pub(crate) above: bool,
}

impl Placement {
    /// The placement `window/place`'s parameters ask for; the schema has checked their types
    /// and that the size is at least 1 (BRIDGE-4.2).
    fn of(params: &Value) -> Option<Self> {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "desktop coordinates in points, far inside f32's range"
        )]
        let number = |name: &str| params.get(name).and_then(Value::as_f64).map(|n| n as f32);
        Some(Self {
            x: number("x")?,
            y: number("y")?,
            width: number("width")?,
            height: number("height")?,
            above: params.get("above").and_then(Value::as_bool)?,
        })
    }
}

/// Whether the server clock runs (BRIDGE-4.9, "pause, resume").
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
    Breakpoint,
    Reload,
}

impl Reason {
    const fn name(self) -> &'static str {
        match self {
            Self::Pause => "pause",
            Self::Step => "step",
            Self::Error => "error",
            Self::Breakpoint => "breakpoint",
            Self::Reload => "reload",
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
    /// The debugger stopped it, which paused the game mid-frame (WORLD-9.1).
    Stopped,
}

/// An event for the bridge to send: its method and parameters (BRIDGE-3.1).
pub(crate) type Event = (&'static str, Value);

/// Where the window's view looks from, for `pick` (BRIDGE-4.9).
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

/// What the host does for a command the console met (PHYS-13.6).
#[derive(Clone, Debug)]
pub(crate) enum HostAction {
    /// Loads a level.
    Level(NextLevel),
    /// Saves the game as `<name>.sav` in the game directory.
    Save(String),
    /// Loads the game in `<name>.sav`.
    Load(String),
    /// Stops.
    Quit,
}

/// The level to load next, what the player carries into it (PHYS-10.4) and what the host
/// keeps from this one (MOVE-6.3, PHYS-12.3).
#[derive(Clone, Debug)]
pub(crate) struct NextLevel {
    pub(crate) map: String,
    pub(crate) carried: Option<SpawnParms>,
    pub(crate) host: HostState,
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
    /// Where `window/place` last asked the window to go, not yet applied.
    placement: Option<Placement>,
    /// The responses of commands answered later, to send.
    responses: Vec<Response>,
    /// Whether the host was asked to stop (BRIDGE-2.5).
    stopped: bool,
    /// Whether frames are drawn.
    screen: Screen,
    /// The source line of each statement, from the progs' `.lno`; empty without one.
    lines: Vec<i32>,
    /// The breakpoints the client set, and the next id to give.
    breakpoints: Vec<Breakpoint>,
    next_breakpoint: u64,
    /// The world before the frame the debugger may stop (WORLD-9.2).
    snapshot: Option<Snapshot>,
    /// Where the frame stopped, while it is stopped.
    halted: Option<Stopped>,
    /// Reads a file of the game's search path, for `reload`; `None` until the host gives it.
    files: Option<qs_sim::ModelFiles>,
    /// The name the progs was read by, which `reload` reads again by default.
    progs_path: String,
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
            placement: None,
            responses: Vec::new(),
            stopped: false,
            screen: Screen::Window,
            lines: Vec::new(),
            breakpoints: Vec::new(),
            next_breakpoint: 0,
            snapshot: None,
            halted: None,
            files: None,
            progs_path: "progs.dat".to_owned(),
        }
    }

    /// The reader of the game's search path, which `reload` reads the progs and its `.lno`
    /// with (BRIDGE-4.9).
    pub(crate) fn set_files(&mut self, files: qs_sim::ModelFiles) {
        self.files = Some(files);
    }

    /// The source line of each statement, from the `.lno` beside the progs (LNO-1).
    pub(crate) fn set_lines(&mut self, lines: Vec<i32>) {
        self.lines = lines;
    }

    /// Takes over what `old`, the level this one replaces, still owes the bridge's client:
    /// its events, before this level's own, the responses it made later and the screenshots
    /// waiting for a frame, so a command whose frame changed the level is still answered.
    pub(crate) fn carry_from(&mut self, mut old: Self) {
        let mine = core::mem::take(&mut self.events);
        self.events = core::mem::take(&mut old.events);
        self.events.extend(mine);
        self.responses.splice(0..0, old.responses);
        self.shots.splice(0..0, old.shots);
        self.stopped |= old.stopped;
        // The breakpoints stay set on the next level, which runs the same progs.
        self.lines = old.lines;
        self.next_breakpoint = old.next_breakpoint;
        for breakpoint in old.breakpoints {
            self.debugger().set_breakpoint(breakpoint.statement, true);
            self.breakpoints.push(breakpoint);
        }
    }

    /// Makes this a game no window draws (`--headless`).
    pub(crate) const fn set_screen(&mut self, screen: Screen) {
        self.screen = screen;
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

    /// Where `window/place` asked the window to go since the last call.
    pub(crate) const fn take_placement(&mut self) -> Option<Placement> {
        self.placement.take()
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

    /// Answers every waiting `screenshot` with `why` the frame could not be drawn.
    pub(crate) fn screenshot_failed(&mut self, why: &str) {
        for shot in core::mem::take(&mut self.shots) {
            let error = RpcError::new(ErrorCode::InternalError, why.to_owned());
            self.responses.push((shot.id, Err(error)));
        }
    }

    /// The level the progs asked the host for since the last call, and what the player
    /// carries into it: after `changelevel`, the next level with the parms
    /// `SV_SaveSpawnparms` keeps (PHYS-10.4); after `restart`, this level with the parms it
    /// was entered with (PHYS-10.6). A level change whose `SetChangeParms` fails stops the
    /// game instead.
    pub(crate) fn host_action(&mut self) -> Option<HostAction> {
        let host = self.world.host_state();
        let action = match self.world.take_host_command()? {
            HostCommand::Restart => HostAction::Level(NextLevel {
                map: self.world.map().to_owned(),
                carried: self.world.carried(),
                host,
            }),
            // PHYS-13.6: a new game, carrying nothing.
            HostCommand::Map(map) => HostAction::Level(NextLevel {
                map,
                carried: None,
                host,
            }),
            HostCommand::Changelevel(map) => {
                let kept = self
                    .with_progs(|game, progs| game.world.save_spawn_parms(progs))
                    .map_err(|error| WorldError::Progs(error.message))
                    .and_then(|kept| kept);
                match kept {
                    Ok(kept) => HostAction::Level(NextLevel {
                        map,
                        carried: Some(kept),
                        host,
                    }),
                    Err(error) => {
                        self.fail(None, &error);
                        return None;
                    }
                }
            }
            HostCommand::Save(name) => HostAction::Save(name),
            HostCommand::Load(name) => HostAction::Load(name),
            HostCommand::Quit => HostAction::Quit,
        };
        Some(action)
    }

    /// The game saved, `save_game` (PHYS-10.5): its text, or why there is none, printed to
    /// the console as id's `Host_Savegame_f` prints it.
    pub(crate) fn save(&mut self) -> Option<Vec<u8>> {
        let saved = self
            .with_progs(|game, progs| game.world.save_game(progs))
            .ok()?;
        match saved {
            Ok(text) => Some(text),
            Err(error) => {
                self.console_line(&format!("{error}\n"));
                None
            }
        }
    }

    /// A line on the console, as `console/line` (BRIDGE-3.1).
    pub(crate) fn console_line(&mut self, text: &str) {
        let time = self.world.time();
        self.events.push((
            "console/line",
            json!({ "text": text.trim_end_matches('\n'), "level": "print", "time": time }),
        ));
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
        let entities = (0..self.world.num_edicts())
            .filter(|&n| self.world.in_use(n))
            .count();
        self.progs_path = path.to_owned();
        self.progs_loaded(path);
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
        match ran {
            // A frame stopped at a statement has not run yet: the client's clock stands.
            Ok(FrameEnd::Stopped) => false,
            Ok(_) => true,
            Err(error) => {
                self.fail(None, &WorldError::Progs(error.message));
                true
            }
        }
    }

    /// Moves the player to `origin`, facing `angles`, at rest, as `player/spawn_at` does
    /// (BRIDGE-4.9): for `--view-script`'s `place` (VIEWDIFF-1.2).
    ///
    /// # Errors
    ///
    /// Why the progs did not read or the player could not be moved.
    pub(crate) fn place_player(
        &mut self,
        origin: [f32; 3],
        angles: [f32; 3],
    ) -> Result<(), String> {
        self.with_progs(|game, progs| game.world.move_player(progs, origin, angles))
            .map_err(|error| error.message)?
            .map_err(|error| error.to_string())
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

    /// The `qc/error` of a failed frame, then the pause it causes (BRIDGE-3.3).
    fn fail(&mut self, progs: Option<&Progs<'_>>, error: &WorldError) {
        let time = self.world.time();
        let mut params = Map::new();
        params.insert("message".to_owned(), json!(error.to_string()));
        params.insert("time".to_owned(), json!(time));
        let mut location = None;
        if let WorldError::Frame(run) = error {
            // BRIDGE-3.4: with a .lno each frame names its file and line. The progs are the
            // caller's, or read again when it holds none.
            let bytes = self.progs.clone();
            let read = Progs::read(&bytes).ok();
            if let Some(progs) = progs.or(read.as_ref()) {
                let stack = self.stack_json(progs, &run.trace);
                location = run
                    .trace
                    .iter()
                    .find(|entry| entry.function > 0)
                    .map(|inner| self.location_json(progs, inner.function, inner.statement));
                if let Some(location) = &location {
                    params.insert("location".to_owned(), location.clone());
                }
                params.insert("stack".to_owned(), Value::Array(stack));
            }
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

    /// BRIDGE-4.6: a command that changes the world at once is refused while a frame is
    /// stopped at a statement, since going on replays the frame from before it (WORLD-9.2).
    fn require_between_frames(&self) -> Result<(), RpcError> {
        match self.halted {
            None => Ok(()),
            Some(_) => Err(RpcError::new(
                ErrorCode::StoppedInFrame,
                "a frame is stopped at a statement; step or resume to its end first",
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
            // Paused between two frames no QuakeC is running and the stack is empty; at a stop
            // it is the stopped run's (WORLD-9.1).
            Some("stack") => {
                self.require_paused()?;
                Ok(json!({ "stack": self.stack_now(progs), "time": time }))
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
        let step = params
            .get("kind")
            .and_then(Value::as_str)
            .and_then(Step::from_name)
            .ok_or_else(|| RpcError::new(ErrorCode::InvalidParams, "no such step"))?;
        self.debug_step(progs, step, STEP_FRAME);
        Ok(self.time())
    }

    /// `console` (BRIDGE-4.9, PHYS-13): the text goes in the console's buffer and runs now,
    /// each line printed coming back as `console/line`.
    fn console(&mut self, progs: &Progs<'_>, params: &Value) -> Value {
        let text = params
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or_default();
        self.world.console(text);
        self.world.console("\n");
        self.world.execute_console(progs);
        self.report_console();
        json!({})
    }

    /// The console's printed lines since the last report, as `console/line` (BRIDGE-3.1); the
    /// lines, for the console on the screen.
    pub(crate) fn report_console(&mut self) -> Vec<Vec<u8>> {
        let lines = self.world.take_console_output();
        for line in &lines {
            let text = String::from_utf8_lossy(line).into_owned();
            self.console_line(&text);
        }
        lines
    }

    /// A line typed into the console on the screen run through qs-sim's console path, as
    /// `Key_Console`'s `Cbuf_AddText` and the next frame's `Cbuf_Execute` run it (CON-4.2); what
    /// it printed.
    pub(crate) fn type_console(&mut self, line: &[u8]) -> Vec<Vec<u8>> {
        self.world.console(&String::from_utf8_lossy(line));
        self.world.console("\n");
        if self
            .with_progs(|game, progs| game.world.execute_console(progs))
            .is_err()
        {
            tracing::warn!("the progs did not read for the console");
        }
        self.report_console()
    }

    /// The command or setting `partial` completes to (CON-4.3).
    pub(crate) fn complete(&self, partial: &[u8]) -> Option<Vec<u8>> {
        self.world
            .complete(&String::from_utf8_lossy(partial))
            .map(String::into_bytes)
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
        Capabilities::of(
            COMMANDS
                .into_iter()
                .filter(|command| self.screen.offers(command)),
        )
    }

    fn call(&mut self, id: &Id, method: &str, params: &Value) -> Answer {
        // A reload replaces the progs every other command reads through.
        if method == "reload" {
            return Answer::Now(self.reload(params));
        }
        if method == "window/place" {
            return Answer::Now(match Placement::of(params) {
                Some(placement) => {
                    self.placement = Some(placement);
                    Ok(Value::Null)
                }
                None => Err(RpcError::new(
                    ErrorCode::InvalidParams,
                    "window/place takes x, y, width, height and above".to_owned(),
                )),
            });
        }
        if method == "screenshot" && self.screen.draws() {
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
                // WORLD-9.2: a stopped frame goes on first; it may stop again.
                if self.state == State::Paused && self.resume_from_stop(progs) {
                    self.apply_held(progs);
                    self.state = State::Running;
                    self.state_changed(None, None);
                }
                Ok(self.time())
            }
            "breakpoint/set" => self.breakpoint_set(progs, params),
            "breakpoint/clear" => self.breakpoint_clear(params),
            "evaluate" => self.evaluate(progs, params),
            "step" => self.step(progs, params),
            "field/set" => self.field_set(progs, params),
            "entity/spawn" => {
                self.require_between_frames()?;
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
                self.require_between_frames()?;
                let edict = edict_param(params)?;
                self.world
                    .remove_entity(progs, edict)
                    .map_err(|e| edit_error(&e))?;
                self.report_changes(progs);
                Ok(json!({}))
            }
            "entity/edit" => self.entity_edit(progs, params),
            "player/spawn_at" => {
                self.require_between_frames()?;
                let origin = vector_param(params, "origin");
                let angles = vector_param(params, "angles");
                self.world
                    .move_player(progs, origin, angles)
                    .map_err(|e| edit_error(&e))?;
                self.report_changes(progs);
                Ok(json!({ "edict": qs_sim::CLIENT_EDICT }))
            }
            "console" => {
                self.require_between_frames()?;
                Ok(self.console(progs, params))
            }
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
    /// and height from the top left, and where the ray from the eye stops (BRIDGE-4.9).
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
pub(crate) mod tests;
