// SPDX-License-Identifier: GPL-3.0-or-later

//! The bridge server (BRIDGE-1.1): one client on a local socket, its messages read on a thread
//! of their own and handed to the game loop, which answers them between two server frames
//! (BRIDGE-4.3) and sends the events.

use std::io::{BufReader, Read, Write};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};

use qs_bridge::{Host, Message, Session, read_frame, write_frame};
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

    /// Listens at `path` and serves the first client (BRIDGE-1.1): a second connection is
    /// closed at once.
    ///
    /// # Errors
    ///
    /// Whatever binding or accepting fails with.
    #[cfg(unix)]
    pub(crate) fn listen(path: &std::path::Path) -> std::io::Result<Self> {
        use std::os::unix::net::UnixListener;
        // A socket file a host before this one left behind.
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        let listener = UnixListener::bind(path)?;
        let (stream, _) = listener.accept()?;
        std::thread::spawn(move || {
            for second in listener.incoming() {
                drop(second);
            }
        });
        let write = stream.try_clone()?;
        Ok(Self::over(stream, write))
    }

    /// Whether the client is still connected.
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

    /// Sends the event `method` with `params`, once the session is initialized (BRIDGE-2.2).
    pub(crate) fn event(&mut self, method: &str, params: Value) {
        if self.session.is_initialized() {
            self.send(&qs_bridge::event(method, params));
        }
    }

    fn send(&mut self, message: &Message) {
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
