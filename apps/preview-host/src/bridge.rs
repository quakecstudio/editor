// SPDX-License-Identifier: GPL-3.0-or-later

//! The bridge server (BRIDGE-1.1): one client on a local socket, its messages read on a thread
//! of their own and handed to the game loop, which answers them between two server frames
//! (BRIDGE-4.3) and sends the events.

use std::io::{self, BufReader, Read, Write};
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};

use interprocess::local_socket::traits::{Listener as _, ListenerExt as _};
use interprocess::local_socket::{GenericFilePath, ListenerOptions, Stream, ToFsName as _};
use qs_bridge::{Host, Id, Message, RpcError, Session, read_frame, write_frame};
use serde_json::Value;

/// What the reader thread hands the game loop.
#[derive(Debug)]
enum Incoming {
    /// A message from the client.
    Message(Message),
    /// The error response to a message that did not read (BRIDGE-1.4).
    Unreadable(Message),
    /// The connection closed, or a frame did not read; the host exits (BRIDGE-1.1).
    Closed,
}

/// The game loop's end of the bridge: the session, the client's messages, and the way back.
pub(crate) struct Bridge {
    session: Session,
    incoming: Receiver<Incoming>,
    outgoing: Arc<Mutex<Box<dyn Write + Send>>>,
    open: bool,
}

impl Bridge {
    /// A bridge over a connection read from `read` and written to `write`.
    pub(crate) fn over(
        read: impl Read + Send + 'static,
        write: impl Write + Send + 'static,
    ) -> Self {
        let (sender, incoming) = channel();
        std::thread::spawn(move || read_messages(read, &sender));
        Self {
            session: Session::new(),
            incoming,
            outgoing: Arc::new(Mutex::new(Box::new(write))),
            open: true,
        }
    }

    /// Listens at `path`, a Unix domain socket on macOS and Linux and a named pipe
    /// `\\.\pipe\<name>` on Windows, and serves the first client (BRIDGE-1.1): a second
    /// connection is closed at once. A socket file a host before this one left behind is
    /// replaced.
    ///
    /// # Errors
    ///
    /// Whatever naming, binding or accepting fails with.
    pub(crate) fn listen(path: &Path) -> io::Result<Self> {
        let name = path.as_os_str().to_fs_name::<GenericFilePath>()?;
        let listener = ListenerOptions::new()
            .name(name)
            .try_overwrite(true)
            .create_sync()?;
        let stream = listener.accept()?;
        std::thread::spawn(move || {
            for second in listener.incoming() {
                drop(second);
            }
        });
        let shared = Shared(Arc::new(stream));
        Ok(Self::over(shared.clone(), shared))
    }

    /// Whether the client is still connected.
    #[cfg(test)]
    pub(crate) const fn is_open(&self) -> bool {
        self.open
    }

    /// Answers every message the client has sent, running its commands on `host`; returns
    /// whether the connection is still open.
    pub(crate) fn poll(&mut self, host: &mut dyn Host) -> bool {
        while self.open {
            let incoming = match self.incoming.try_recv() {
                Ok(incoming) => incoming,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => Incoming::Closed,
            };
            self.answer(host, incoming);
        }
        self.open
    }

    /// Waits for the client's next message and answers it, for a test that must not race the
    /// reader thread.
    #[cfg(test)]
    pub(crate) fn handle_next(&mut self, host: &mut dyn Host) {
        let incoming = self.incoming.recv().unwrap_or(Incoming::Closed);
        self.answer(host, incoming);
    }

    /// Answers one thing the reader handed over.
    fn answer(&mut self, host: &mut dyn Host, incoming: Incoming) {
        {
            match incoming {
                Incoming::Message(message) => {
                    let reply = self.session.handle(host, message);
                    if let Some(response) = reply.response {
                        self.send(&response);
                    }
                    if reply.close {
                        self.open = false;
                    }
                }
                Incoming::Unreadable(response) => self.send(&response),
                Incoming::Closed => self.open = false,
            }
        }
    }

    /// Whether the client has initialized the session, so events may be sent (BRIDGE-2.2).
    pub(crate) const fn is_initialized(&self) -> bool {
        self.session.is_initialized()
    }

    /// Sends the response to request `id`, which the host answered later.
    pub(crate) fn respond(&mut self, id: Id, outcome: Result<Value, RpcError>) {
        self.send(&Message::Response {
            id: Some(id),
            outcome,
        });
    }

    /// Sends the event `method` with `params`, once the session is initialized (BRIDGE-2.2).
    pub(crate) fn event(&mut self, method: &str, params: Value) {
        if self.session.is_initialized() {
            self.send(&qs_bridge::event(method, params));
        }
    }

    fn send(&mut self, message: &Message) {
        if !self.open {
            return;
        }
        let sent = self
            .outgoing
            .lock()
            .ok()
            .and_then(|mut out| write_frame(&mut *out, &message.to_bytes()).ok());
        if sent.is_none() {
            tracing::warn!("the bridge client went away");
            self.open = false;
        }
    }
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

/// Reads frames from `read` until it ends, handing each message to the game loop.
fn read_messages(read: impl Read, sender: &Sender<Incoming>) {
    let mut reader = BufReader::new(read);
    loop {
        let incoming = match read_frame(&mut reader) {
            Ok(Some(content)) => match Message::parse(&content) {
                Ok(message) => Incoming::Message(message),
                Err(response) => Incoming::Unreadable(*response),
            },
            Ok(None) => Incoming::Closed,
            Err(error) => {
                tracing::warn!(%error, "a bridge frame did not read");
                Incoming::Closed
            }
        };
        let closed = matches!(incoming, Incoming::Closed);
        if sender.send(incoming).is_err() || closed {
            return;
        }
    }
}

#[cfg(test)]
mod tests;
