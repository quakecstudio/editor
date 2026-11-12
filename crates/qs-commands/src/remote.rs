// SPDX-License-Identifier: GPL-3.0-or-later

//! Commands run in another process's session (AGT-7): the running app hosts its [`Session`]
//! on a local socket, and a [`Client`] in another process, such as `quakec-ide mcp --connect`,
//! lists its commands and runs them there.
//!
//! The wire is one JSON object per line each way. A request is `"commands"` or
//! `{"execute": {"command": <id>, "params": <object>}}`; the answer is
//! `{"commands": [{"id", "summary", "params"}, ...]}`, `{"output": <value>}` or
//! `{"error": <message>}`.
//!
//! The commands run on the host's own thread, when it calls [`Host::answer`] (each frame, in
//! the app), so they see and change the state the window shows.

use std::io::{BufRead as _, BufReader, Write as _};
use std::path::Path;
use std::sync::mpsc;

use interprocess::local_socket::traits::{Listener as _, Stream as _};
use interprocess::local_socket::{GenericFilePath, ListenerOptions, Stream, ToFsName as _};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Session;

/// A command as another process sees it: its id, its summary and its parameters' schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Described {
    id: String,
    summary: String,
    params: Value,
}

impl Described {
    /// The command's id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// What the command does, in one line.
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// The JSON Schema of the command's parameters.
    #[must_use]
    pub const fn params(&self) -> &Value {
        &self.params
    }
}

/// What a client asks.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Request {
    Commands,
    Execute { command: String, params: Value },
}

/// What the host answers.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Answer {
    Commands(Vec<Described>),
    Output(Value),
    Error(String),
}

/// Why the socket could not be served or reached.
#[derive(Debug)]
pub enum RemoteError {
    /// Another process already serves the socket.
    InUse,
    /// The socket could not be made, reached, read or written.
    Io(std::io::Error),
    /// The other side sent something that is not an answer, or closed the connection.
    Protocol(String),
}

impl std::fmt::Display for RemoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InUse => f.write_str("another QuakeC Studio already answers on its socket"),
            Self::Io(error) => write!(f, "the app's socket: {error}"),
            Self::Protocol(why) => write!(f, "the app answered badly: {why}"),
        }
    }
}

impl std::error::Error for RemoteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::InUse | Self::Protocol(_) => None,
        }
    }
}

impl From<std::io::Error> for RemoteError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// One request waiting for the host's thread, and where its answer goes.
struct Call {
    request: Request,
    reply: mpsc::Sender<Answer>,
}

/// The host's side: a socket other processes' clients connect to, whose requests wait until
/// [`Host::answer`] runs them in a session.
pub struct Host {
    calls: mpsc::Receiver<Call>,
}

impl Host {
    /// Listens on `socket`, calling `wake` each time a request arrives so that the host's
    /// thread runs [`Host::answer`] soon (the app asks for a frame).
    ///
    /// A socket file left by a host that has gone is replaced; one a live host answers on is
    /// not.
    ///
    /// # Errors
    ///
    /// [`RemoteError::InUse`] when another host answers on `socket`, and [`RemoteError::Io`]
    /// when the socket cannot be made.
    pub fn listen(
        socket: &Path,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self, RemoteError> {
        if connect(socket).is_ok() {
            return Err(RemoteError::InUse);
        }
        if let Some(folder) = socket.parent()
            && !cfg!(windows)
        {
            std::fs::create_dir_all(folder)?;
        }
        let name = socket.as_os_str().to_fs_name::<GenericFilePath>()?;
        let listener = ListenerOptions::new()
            .name(name)
            .try_overwrite(true)
            .create_sync()?;
        let (calls, inbox) = mpsc::channel();
        let wake = std::sync::Arc::new(wake);
        std::thread::spawn(move || {
            loop {
                let Ok(stream) = listener.accept() else {
                    continue;
                };
                let calls = calls.clone();
                let wake = std::sync::Arc::clone(&wake);
                std::thread::spawn(move || {
                    // A client that hangs up or breaks the protocol only ends its connection.
                    let _ended = connection(stream, &calls, &*wake);
                });
            }
        });
        Ok(Self { calls: inbox })
    }

    /// Runs every request that has arrived in `session`, answering each; returns how many.
    pub fn answer<C: 'static>(&self, session: &mut Session<C>) -> usize {
        let mut answered: usize = 0;
        while let Ok(call) = self.calls.try_recv() {
            call.run(session);
            answered = answered.saturating_add(1);
        }
        answered
    }

    /// Waits for the next request and runs it in `session`; `false` once no client can send
    /// any more.
    pub fn answer_next<C: 'static>(&self, session: &mut Session<C>) -> bool {
        match self.calls.recv() {
            Ok(call) => {
                call.run(session);
                true
            }
            Err(_) => false,
        }
    }
}

impl Call {
    /// Runs `call` in `session` and sends its answer back to the connection's thread.
    fn run<C: 'static>(self, session: &mut Session<C>) {
        let answer = match self.request {
            Request::Commands => Answer::Commands(
                session
                    .registry()
                    .commands()
                    .map(|info| Described {
                        id: info.id().to_string(),
                        summary: info.summary().to_owned(),
                        params: info.params().clone(),
                    })
                    .collect(),
            ),
            Request::Execute { command, params } => match session.execute(&command, params) {
                Ok(output) => Answer::Output(output),
                Err(error) => Answer::Error(error.to_string()),
            },
        };
        // A connection that closed no longer waits for its answer.
        let _sent = self.reply.send(answer);
    }
}

/// Serves one client: each line a request, passed to the host's thread, its answer a line back.
fn connection(
    stream: Stream,
    calls: &mpsc::Sender<Call>,
    wake: &dyn Fn(),
) -> Result<(), RemoteError> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let answer = match serde_json::from_str::<Request>(&line) {
            Ok(request) => {
                let (reply, answered) = mpsc::channel();
                calls
                    .send(Call { request, reply })
                    .map_err(|_| RemoteError::Protocol("the host stopped".to_owned()))?;
                wake();
                answered
                    .recv()
                    .map_err(|_| RemoteError::Protocol("the host stopped".to_owned()))?
            }
            Err(error) => Answer::Error(format!("not a request: {error}")),
        };
        write_line(reader.get_mut(), &answer)?;
    }
}

/// Writes `value` as one line of JSON.
fn write_line(stream: &mut Stream, value: &impl Serialize) -> Result<(), RemoteError> {
    let mut text =
        serde_json::to_string(value).map_err(|error| RemoteError::Protocol(error.to_string()))?;
    text.push('\n');
    stream.write_all(text.as_bytes())?;
    stream.flush()?;
    Ok(())
}

/// A connection to `socket`.
fn connect(socket: &Path) -> std::io::Result<Stream> {
    socket
        .as_os_str()
        .to_fs_name::<GenericFilePath>()
        .and_then(Stream::connect)
}

/// The client's side: a connection to a host's socket.
pub struct Client {
    stream: BufReader<Stream>,
}

impl Client {
    /// Connects to the host answering on `socket`.
    ///
    /// # Errors
    ///
    /// [`RemoteError::Io`] when no host answers there.
    pub fn connect(socket: &Path) -> Result<Self, RemoteError> {
        Ok(Self {
            stream: BufReader::new(connect(socket)?),
        })
    }

    /// The host's commands.
    ///
    /// # Errors
    ///
    /// [`RemoteError`] when the connection fails or the answer is not a list of commands.
    pub fn commands(&mut self) -> Result<Vec<Described>, RemoteError> {
        match self.ask(&Request::Commands)? {
            Answer::Commands(commands) => Ok(commands),
            other => Err(other.unexpected()),
        }
    }

    /// Runs the command `id` with `params` in the host's session: its output, or
    /// `Ok(Err(message))` when the command failed there.
    ///
    /// # Errors
    ///
    /// [`RemoteError`] when the connection fails or the answer is not one.
    pub fn execute(
        &mut self,
        id: &str,
        params: Value,
    ) -> Result<Result<Value, String>, RemoteError> {
        let request = Request::Execute {
            command: id.to_owned(),
            params,
        };
        match self.ask(&request)? {
            Answer::Output(output) => Ok(Ok(output)),
            Answer::Error(why) => Ok(Err(why)),
            other @ Answer::Commands(_) => Err(other.unexpected()),
        }
    }

    /// Sends `request` and reads its answer.
    fn ask(&mut self, request: &Request) -> Result<Answer, RemoteError> {
        write_line(self.stream.get_mut(), request)?;
        let mut line = String::new();
        if self.stream.read_line(&mut line)? == 0 {
            return Err(RemoteError::Protocol(
                "the app closed the connection".to_owned(),
            ));
        }
        serde_json::from_str(&line).map_err(|error| RemoteError::Protocol(error.to_string()))
    }
}

impl Answer {
    /// An answer that does not fit the request.
    fn unexpected(&self) -> RemoteError {
        RemoteError::Protocol(format!("unexpected answer {self:?}"))
    }
}
