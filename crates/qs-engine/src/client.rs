// SPDX-License-Identifier: GPL-3.0-or-later

//! The bridge client (BRIDGE-1 to BRIDGE-7): requests framed and numbered, each answered by
//! the response with its id; the host's events kept, in order, as they come between them; and
//! one method per command, refused before it is sent when the host's capabilities leave it
//! out (BRIDGE-2.10).

use qc_edict::EdictNumber;
use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::io::{self, BufReader, Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError, channel};
use std::time::{Duration, Instant};

use interprocess::local_socket::traits::Stream as _;
use interprocess::local_socket::{GenericFilePath, Stream, ToFsName as _};
use qs_bridge::{
    Applied, BRIDGE_PROTOCOL_VERSION, ErrorCode, Id, Message, is_flagged, read_frame, write_frame,
};
use serde_json::{Value, json};

/// An event the host sent (BRIDGE-3.1): its method and parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct BridgeEvent {
    /// `progs/loaded`, `edict/changed` and the others.
    pub method: String,
    /// Its parameters, as its schema gives them.
    pub params: Value,
}

/// Why a request did not get its result.
#[derive(Debug)]
pub enum ClientError {
    /// The connection failed.
    Io(io::Error),
    /// The host closed the connection, or sent what is not the protocol.
    Closed,
    /// The host answered with an error (BRIDGE-7.1).
    Rpc {
        /// Its code.
        code: ErrorCode,
        /// Its message, for the person.
        message: String,
    },
    /// The host's capabilities leave the command out (BRIDGE-2.10).
    Unsupported(String),
    /// No answer came within the time given.
    Timeout,
    /// The host has not answered an earlier request, so this one was not sent: a host that
    /// stopped answering is not waited for again.
    NotAnswering,
    /// The host's answer to the command, named here, is not what the command answers.
    Unreadable(&'static str),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "the bridge: {error}"),
            Self::Closed => f.write_str("the preview host closed the bridge"),
            Self::Rpc { code, message } => {
                write!(f, "the preview host: {message} ({})", code.code())
            }
            Self::Unsupported(method) => {
                write!(f, "the preview host does not offer {method}")
            }
            Self::Timeout => f.write_str("the preview host did not answer in time"),
            Self::NotAnswering => f.write_str(
                "the preview host is not answering: it has not answered an earlier command",
            ),
            Self::Unreadable(method) => {
                write!(f, "the preview host's answer to {method} does not read")
            }
        }
    }
}

impl std::error::Error for ClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Closed
            | Self::Rpc { .. }
            | Self::Unsupported(_)
            | Self::Timeout
            | Self::NotAnswering
            | Self::Unreadable(_) => None,
        }
    }
}

/// What `step` runs (BRIDGE-4.9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepKind {
    /// One statement.
    Statement,
    /// Into the next call.
    In,
    /// To the next statement of this function.
    Over,
    /// To the caller.
    Out,
    /// To the end of the server frame.
    Frame,
}

impl StepKind {
    const fn name(self) -> &'static str {
        match self {
            Self::Statement => "statement",
            Self::In => "in",
            Self::Over => "over",
            Self::Out => "out",
            Self::Frame => "frame",
        }
    }
}

/// Where `window/place` puts the host's window (BRIDGE-4.10): its outer frame in the desktop's
/// logical points, from the top left of the main display, and whether it stays above other
/// windows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowPlacement {
    /// The frame's left edge.
    pub x: f32,
    /// Its top edge.
    pub y: f32,
    /// Its width, at least 1.
    pub width: f32,
    /// Its height, at least 1.
    pub height: f32,
    /// Whether it stays above other windows.
    pub above: bool,
}

/// What `inspect` reads (BRIDGE-4.9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectWhat {
    /// One edict's classname and fields.
    Edict(EdictNumber),
    /// Every named global.
    Globals,
    /// The call stack, while paused.
    Stack,
    /// The edicts in use.
    Entities,
}

/// How long a pumped client waits for a message after each pump of its host (ADR-0028).
const PUMP_WAIT: Duration = Duration::from_millis(1);

/// How long [`Client::request`] waits for its response.
const ANSWER_WITHIN: Duration = Duration::from_secs(30);

/// The entity an edit or a removal names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityTarget {
    /// An edict by its number (BRIDGE-5.2).
    Edict(EdictNumber),
    /// An entity by its place in the map editor's list of the map's entities (BRIDGE-4.13).
    MapEntity(usize),
}

impl EntityTarget {
    /// The parameter that names it.
    fn to_json(self) -> Value {
        match self {
            Self::Edict(edict) => json!({ "edict": edict.index() }),
            Self::MapEntity(place) => json!({ "map_entity": place }),
        }
    }
}

/// A connection to the preview host's bridge.
pub struct Client {
    write: Box<dyn Write + Send>,
    incoming: Receiver<Option<Message>>,
    next_id: i64,
    events: VecDeque<BridgeEvent>,
    capabilities: BTreeMap<String, bool>,
    /// For a host in this process (ADR-0028): lets it answer while the client waits.
    pump: Option<Box<dyn FnMut()>>,
    /// The request the host has not answered in time, if any; until its answer comes no
    /// other is sent.
    overdue: Option<i64>,
    /// How long a request waits for its answer.
    answer_within: Duration,
}

impl Client {
    /// Connects to the host listening at `path`, a Unix domain socket or a named pipe
    /// `\\.\pipe\<name>` (BRIDGE-1.1), trying `attempts` times `pause` apart while the host is
    /// starting.
    ///
    /// # Errors
    ///
    /// [`ClientError::Io`] when the last attempt fails.
    pub fn connect(path: &Path, attempts: u32, pause: Duration) -> Result<Self, ClientError> {
        let mut last = io::Error::other("no attempt");
        for _ in 0..attempts.max(1) {
            match path
                .as_os_str()
                .to_fs_name::<GenericFilePath>()
                .and_then(Stream::connect)
            {
                Ok(stream) => {
                    let shared = Shared(Arc::new(stream));
                    return Ok(Self::over(shared.clone(), shared));
                }
                Err(error) => last = error,
            }
            std::thread::sleep(pause);
        }
        Err(ClientError::Io(last))
    }

    /// A client over a connection read from `read` and written to `write`.
    pub fn over(read: impl Read + Send + 'static, write: impl Write + Send + 'static) -> Self {
        let (sender, incoming) = channel();
        std::thread::spawn(move || read_messages(read, &sender));
        Self {
            write: Box::new(write),
            incoming,
            next_id: 0,
            events: VecDeque::new(),
            capabilities: BTreeMap::new(),
            pump: None,
            overdue: None,
            answer_within: ANSWER_WITHIN,
        }
    }

    /// Waits `within` for each answer from now on instead of the default, as a test of a host
    /// that does not answer waits for none.
    pub const fn answer_within(&mut self, within: Duration) {
        self.answer_within = within;
    }

    /// The client of a host that runs in this process, on this thread (ADR-0028): while the
    /// client waits for a message it calls `pump`, which lets the host answer its bridge, since
    /// no other thread runs the host.
    #[must_use]
    pub fn pumped(mut self, pump: Box<dyn FnMut()>) -> Self {
        self.pump = Some(pump);
        self
    }

    /// `initialize` (BRIDGE-2.1, BRIDGE-2.2), naming this client `name`: the host's answer,
    /// whose capabilities the commands are checked against from now on.
    ///
    /// # Errors
    ///
    /// [`ClientError`] when the host refuses or does not answer.
    pub fn initialize(&mut self, name: &str) -> Result<Value, ClientError> {
        let (major, minor) = BRIDGE_PROTOCOL_VERSION;
        let result = self.send(
            "initialize",
            json!({
                "protocolVersion": format!("{major}.{minor}"),
                "client": { "name": name, "version": env!("CARGO_PKG_VERSION") },
            }),
        )?;
        if let Some(Value::Object(capabilities)) = result.get("capabilities") {
            self.capabilities = capabilities
                .iter()
                .map(|(name, on)| (name.clone(), on.as_bool().unwrap_or(false)))
                .collect();
        }
        Ok(result)
    }

    /// Whether the host offers `method` (BRIDGE-2.10): every command a capability does not
    /// flag, and each flagged one the host said it has.
    #[must_use]
    pub fn offers(&self, method: &str) -> bool {
        !is_flagged(method) || self.capabilities.get(method).copied().unwrap_or(false)
    }

    /// Sends the command `method` with `params` and waits for its result, keeping the events
    /// that come before it.
    ///
    /// # Errors
    ///
    /// [`ClientError::Unsupported`] before sending a command the host does not offer, and the
    /// other [`ClientError`]s for its answer.
    pub fn request(&mut self, method: &str, params: Value) -> Result<Value, ClientError> {
        self.check_answering()?;
        if !self.offers(method) {
            return Err(ClientError::Unsupported(method.to_owned()));
        }
        self.send(method, params)
    }

    /// [`ClientError::NotAnswering`] while a request is overdue, after reading, without
    /// waiting, whether its answer has come.
    fn check_answering(&mut self) -> Result<(), ClientError> {
        if self.overdue.is_none() {
            return Ok(());
        }
        let events = self.poll_events()?;
        self.events.extend(events);
        if self.overdue.is_some() {
            return Err(ClientError::NotAnswering);
        }
        Ok(())
    }

    fn send(&mut self, method: &str, params: Value) -> Result<Value, ClientError> {
        self.check_answering()?;
        self.next_id = self.next_id.saturating_add(1);
        let id = self.next_id;
        let message = Message::Request {
            id: Id::Number(id),
            method: method.to_owned(),
            params,
        };
        write_frame(&mut self.write, &message.to_bytes()).map_err(ClientError::Io)?;
        let deadline = deadline_after(self.answer_within);
        loop {
            let message = match self.next_message(deadline) {
                Ok(message) => message,
                Err(ClientError::Timeout) => {
                    self.overdue = Some(id);
                    return Err(ClientError::Timeout);
                }
                Err(error) => return Err(error),
            };
            match message {
                Message::Response {
                    id: Some(Id::Number(answered)),
                    outcome,
                } if answered == id => {
                    return outcome.map_err(|error| ClientError::Rpc {
                        code: error.code,
                        message: error.message,
                    });
                }
                Message::Notification { method, params } => {
                    self.events.push_back(BridgeEvent { method, params });
                }
                // A response to another request, or a request from the host, which this
                // protocol never sends, is not this one's answer.
                Message::Response { .. } | Message::Request { .. } => {}
            }
        }
    }

    /// The next message, waiting until `deadline`: the whole wait's limit, so a host that
    /// keeps sending other messages does not stretch it.
    fn next_message(&mut self, deadline: Instant) -> Result<Message, ClientError> {
        if let Some(pump) = &mut self.pump {
            // The host answers only when pumped, and its answer crosses two reader threads, so
            // each pump is followed by a short wait for it rather than a spin.
            loop {
                pump();
                match self.incoming.recv_timeout(PUMP_WAIT) {
                    Ok(Some(message)) => return Ok(message),
                    Ok(None) | Err(RecvTimeoutError::Disconnected) => {
                        return Err(ClientError::Closed);
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                }
                if Instant::now() >= deadline {
                    return Err(ClientError::Timeout);
                }
            }
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(ClientError::Timeout);
        }
        match self.incoming.recv_timeout(left) {
            Ok(Some(message)) => Ok(message),
            Ok(None) | Err(RecvTimeoutError::Disconnected) => Err(ClientError::Closed),
            Err(RecvTimeoutError::Timeout) => Err(ClientError::Timeout),
        }
    }

    /// Whether the host answers: false once a request went unanswered in time, until its
    /// answer comes.
    #[must_use]
    pub const fn is_answering(&self) -> bool {
        self.overdue.is_none()
    }

    /// The events received so far, oldest first.
    pub fn take_events(&mut self) -> Vec<BridgeEvent> {
        self.events.drain(..).collect()
    }

    /// The events received so far and those waiting to be read, oldest first, without waiting
    /// for more; `Err(ClientError::Closed)` once the host closed the connection and every event
    /// before it was taken.
    ///
    /// # Errors
    ///
    /// [`ClientError::Closed`] when the connection is closed and no event is left.
    pub fn poll_events(&mut self) -> Result<Vec<BridgeEvent>, ClientError> {
        let mut closed = false;
        loop {
            match self.incoming.try_recv() {
                Ok(Some(Message::Notification { method, params })) => {
                    self.events.push_back(BridgeEvent { method, params });
                }
                Ok(Some(Message::Response {
                    id: Some(Id::Number(answered)),
                    ..
                })) if self.overdue == Some(answered) => self.overdue = None,
                Ok(Some(Message::Response { .. } | Message::Request { .. })) => {}
                Ok(None) | Err(TryRecvError::Disconnected) => {
                    closed = true;
                    break;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        if closed && self.events.is_empty() {
            return Err(ClientError::Closed);
        }
        Ok(self.take_events())
    }

    /// Waits up to `within` for the event `method`, keeping every other event; the events
    /// already received are looked at first.
    ///
    /// # Errors
    ///
    /// [`ClientError::Timeout`] or [`ClientError::Closed`] when it does not come.
    pub fn wait_event(
        &mut self,
        method: &str,
        within: Duration,
    ) -> Result<BridgeEvent, ClientError> {
        if let Some(at) = self.events.iter().position(|e| e.method == method)
            && let Some(event) = self.events.remove(at)
        {
            return Ok(event);
        }
        let deadline = deadline_after(within);
        loop {
            match self.next_message(deadline)? {
                Message::Notification { method: m, params } if m == method => {
                    return Ok(BridgeEvent { method: m, params });
                }
                Message::Notification { method: m, params } => {
                    self.events.push_back(BridgeEvent { method: m, params });
                }
                Message::Response { .. } | Message::Request { .. } => {}
            }
        }
    }

    /// `pause` (BRIDGE-4.9); the server's time.
    ///
    /// # Errors
    ///
    /// The request's [`ClientError`].
    pub fn pause(&mut self) -> Result<Value, ClientError> {
        self.request("pause", json!({}))
    }

    /// `resume` (BRIDGE-4.9).
    ///
    /// # Errors
    ///
    /// The request's [`ClientError`].
    pub fn resume(&mut self) -> Result<Value, ClientError> {
        self.request("resume", json!({}))
    }

    /// `step` (BRIDGE-4.9, BRIDGE-4.3).
    ///
    /// # Errors
    ///
    /// The request's [`ClientError`].
    pub fn step(&mut self, kind: StepKind) -> Result<Value, ClientError> {
        self.request("step", json!({ "kind": kind.name() }))
    }

    /// `inspect` (BRIDGE-4.9).
    ///
    /// # Errors
    ///
    /// The request's [`ClientError`].
    pub fn inspect(&mut self, what: InspectWhat) -> Result<Value, ClientError> {
        let params = match what {
            InspectWhat::Edict(edict) => json!({ "what": "edict", "edict": edict.index() }),
            InspectWhat::Globals => json!({ "what": "globals" }),
            InspectWhat::Stack => json!({ "what": "stack" }),
            InspectWhat::Entities => json!({ "what": "entities" }),
        };
        self.request("inspect", params)
    }

    /// `console` (BRIDGE-4.9); its output comes as `console/line` events.
    ///
    /// # Errors
    ///
    /// The request's [`ClientError`].
    pub fn console(&mut self, command: &str) -> Result<Value, ClientError> {
        self.request("console", json!({ "command": command }))
    }

    /// `screenshot` (BRIDGE-4.9), at the size asked or the size drawn.
    ///
    /// # Errors
    ///
    /// The request's [`ClientError`].
    pub fn screenshot(&mut self, size: Option<(u32, u32)>) -> Result<Value, ClientError> {
        let params = size.map_or_else(
            || json!({}),
            |(width, height)| json!({ "width": width, "height": height }),
        );
        self.request("screenshot", params)
    }

    /// [`Client::screenshot`]'s PNG, decoded from its base64 (BRIDGE-4.9).
    ///
    /// # Errors
    ///
    /// The request's [`ClientError`], or [`ClientError::Unsupported`] naming `screenshot` when
    /// the answer holds no PNG that decodes.
    pub fn screenshot_png(&mut self, size: Option<(u32, u32)>) -> Result<Vec<u8>, ClientError> {
        let answer = self.screenshot(size)?;
        answer
            .get("png")
            .and_then(Value::as_str)
            .and_then(qs_bridge::base64_decode)
            .ok_or_else(|| ClientError::Unsupported("screenshot".to_owned()))
    }

    /// `field/set` (BRIDGE-4.4), `value` a bridge value (BRIDGE-5.1); when the value applies.
    ///
    /// # Errors
    ///
    /// The request's [`ClientError`], and [`ClientError::Unreadable`] for an answer that says
    /// neither `now` nor `on_resume`.
    pub fn set_field(
        &mut self,
        edict: EdictNumber,
        field: &str,
        value: &Value,
    ) -> Result<Applied, ClientError> {
        const METHOD: &str = "field/set";
        let answer = self.request(
            METHOD,
            json!({ "edict": edict.index(), "field": field, "value": value }),
        )?;
        Applied::of_result(&answer).ok_or(ClientError::Unreadable(METHOD))
    }

    /// `pick` (BRIDGE-4.9): `x` and `y` fractions of the window from its top left.
    ///
    /// # Errors
    ///
    /// The request's [`ClientError`].
    pub fn pick(&mut self, x: f64, y: f64) -> Result<Value, ClientError> {
        self.request("pick", json!({ "x": x, "y": y }))
    }

    /// `window/place` (BRIDGE-4.10): moves and sizes the host's window to `placement`.
    ///
    /// # Errors
    ///
    /// The request's [`ClientError`]: [`ClientError::Unsupported`] from a host that cannot place
    /// its window (`--headless`, Wayland).
    pub fn place_window(&mut self, placement: WindowPlacement) -> Result<Value, ClientError> {
        let WindowPlacement {
            x,
            y,
            width,
            height,
            above,
        } = placement;
        self.request(
            "window/place",
            json!({ "x": x, "y": y, "width": width, "height": height, "above": above }),
        )
    }

    /// `entity/spawn` (BRIDGE-4.9), the entity put at `place` in the list of the map's
    /// entities when one is given (BRIDGE-4.13).
    ///
    /// # Errors
    ///
    /// The request's [`ClientError`].
    pub fn spawn(
        &mut self,
        classname: &str,
        keyvalues: &[(&str, &str)],
        place: Option<usize>,
    ) -> Result<Value, ClientError> {
        let mut params = json!({ "classname": classname, "keyvalues": pairs(keyvalues) });
        if let (Some(place), Some(object)) = (place, params.as_object_mut()) {
            object.insert("map_entity".to_owned(), json!(place));
        }
        self.request("entity/spawn", params)
    }

    /// `entity/remove` (BRIDGE-4.9, BRIDGE-4.13).
    ///
    /// # Errors
    ///
    /// The request's [`ClientError`].
    pub fn remove(&mut self, target: EntityTarget) -> Result<Value, ClientError> {
        self.request("entity/remove", target.to_json())
    }

    /// `entity/edit` (BRIDGE-4.4, BRIDGE-4.13).
    ///
    /// # Errors
    ///
    /// The request's [`ClientError`].
    pub fn edit_entity(
        &mut self,
        target: EntityTarget,
        keyvalues: &[(&str, &str)],
    ) -> Result<Value, ClientError> {
        let mut params = target.to_json();
        if let Some(object) = params.as_object_mut() {
            object.insert("keyvalues".to_owned(), pairs(keyvalues));
        }
        self.request("entity/edit", params)
    }

    /// `map/swap` (BRIDGE-4.14): the BSP at `bsp`, a new build of the running map, put in place
    /// of the old, each of `models`, a map entity's place (BRIDGE-4.13) and its brush model
    /// `*n` in the new build, given its model.
    ///
    /// # Errors
    ///
    /// The request's [`ClientError`].
    pub fn swap_map(
        &mut self,
        bsp: &str,
        models: &[(usize, String)],
    ) -> Result<Value, ClientError> {
        let models: Vec<Value> = models
            .iter()
            .map(|(place, model)| json!({ "map_entity": place, "model": model }))
            .collect();
        self.request("map/swap", json!({ "bsp": bsp, "models": models }))
    }

    /// `player/spawn_at` (BRIDGE-4.9).
    ///
    /// # Errors
    ///
    /// The request's [`ClientError`].
    pub fn spawn_player_at(
        &mut self,
        origin: [f32; 3],
        angles: [f32; 3],
    ) -> Result<Value, ClientError> {
        self.request(
            "player/spawn_at",
            json!({ "origin": origin, "angles": angles }),
        )
    }

    /// `player/spawn_at` with `room` `find` (BRIDGE-4.19): the player put at the nearest
    /// height to `origin` where its box is free; where that is.
    ///
    /// # Errors
    ///
    /// The request's [`ClientError`]: [`ErrorCode::InvalidParams`] when no height is free.
    pub fn place_player_near(
        &mut self,
        origin: [f32; 3],
        angles: [f32; 3],
    ) -> Result<[f32; 3], ClientError> {
        let placed = self.request(
            "player/spawn_at",
            json!({ "origin": origin, "angles": angles, "room": "find" }),
        )?;
        Ok(placed
            .get("origin")
            .and_then(|at| serde_json::from_value(at.clone()).ok())
            .unwrap_or(origin))
    }

    /// `reload` (BRIDGE-4.9), of the progs at `path`, or the running one's again.
    ///
    /// # Errors
    ///
    /// The request's [`ClientError`].
    pub fn reload(&mut self, path: Option<&str>) -> Result<Value, ClientError> {
        let params = path.map_or_else(|| json!({}), |path| json!({ "progs": path }));
        self.request("reload", params)
    }

    /// `reload` with `dry_run` (BRIDGE-4.11): what a reload of the progs at `path` would
    /// migrate, lose and add, the running game unchanged.
    ///
    /// # Errors
    ///
    /// The request's [`ClientError`].
    pub fn predict_reload(&mut self, path: &str) -> Result<Value, ClientError> {
        self.request("reload", json!({ "progs": path, "dry_run": true }))
    }

    /// `breakpoint/set` (BRIDGE-4.9) at `line` of `file`, relative to the project's root.
    ///
    /// # Errors
    ///
    /// The request's [`ClientError`].
    pub fn set_breakpoint(&mut self, file: &str, line: u32) -> Result<Value, ClientError> {
        self.request("breakpoint/set", json!({ "file": file, "line": line }))
    }

    /// `breakpoint/clear` (BRIDGE-4.9) of the breakpoint `breakpoint/set` gave `id`.
    ///
    /// # Errors
    ///
    /// The request's [`ClientError`].
    pub fn clear_breakpoint(&mut self, id: BreakpointId) -> Result<Value, ClientError> {
        self.request("breakpoint/clear", json!({ "id": id.get() }))
    }

    /// `evaluate` (BRIDGE-4.9): `expression`'s value while stopped in a frame.
    ///
    /// # Errors
    ///
    /// The request's [`ClientError`].
    pub fn evaluate(&mut self, expression: &str) -> Result<Value, ClientError> {
        self.request("evaluate", json!({ "expression": expression }))
    }

    /// `shutdown` (BRIDGE-2.5): the host stops its game and answers.
    ///
    /// # Errors
    ///
    /// The request's [`ClientError`].
    pub fn shutdown(&mut self) -> Result<Value, ClientError> {
        self.request("shutdown", json!({}))
    }
}

/// `within` from now; one past the clock's reach is now, so the wait times out at once.
fn deadline_after(within: Duration) -> Instant {
    let now = Instant::now();
    now.checked_add(within).unwrap_or(now)
}

/// Key-values as BRIDGE-5.2 writes them.
fn pairs(keyvalues: &[(&str, &str)]) -> Value {
    Value::Array(
        keyvalues
            .iter()
            .map(|(key, value)| json!({ "key": key, "value": value }))
            .collect(),
    )
}

/// One connection for the reader thread and the writer both: `&Stream` reads and writes.
#[derive(Clone)]
struct Shared(Arc<Stream>);

impl Read for Shared {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&*self.0).read(buf)
    }
}

impl Write for Shared {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&*self.0).write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        (&*self.0).flush()
    }
}

/// Reads frames from `read` until it ends, handing each message over; `None` once the
/// connection is gone or sends what does not read.
fn read_messages(read: impl Read, sender: &Sender<Option<Message>>) {
    let mut reader = BufReader::new(read);
    loop {
        let message = match read_frame(&mut reader) {
            Ok(Some(content)) => Message::parse(&content)
                .inspect_err(|answer| {
                    tracing::warn!(?answer, "the host sent a message that does not read");
                })
                .ok(),
            Ok(None) => None,
            // A host stopped or killed often ends its connection with a reset, not an end.
            Err(error) => {
                tracing::debug!(%error, "the host's connection ended");
                None
            }
        };
        let closed = message.is_none();
        if sender.send(message).is_err() || closed {
            return;
        }
    }
}

/// The host's id for a breakpoint it took (BRIDGE-4.9); written to JSON as the bare number.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
#[serde(transparent)]
pub struct BreakpointId(u64);

impl BreakpointId {
    /// The breakpoint the host numbered `id`.
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// The host's number for it.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for BreakpointId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
