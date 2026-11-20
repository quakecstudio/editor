// SPDX-License-Identifier: GPL-3.0-or-later

//! An agent run as a subprocess, spoken to over its standard input and output
//! (`transports.mdx`, stdio: "The client launches the agent as a subprocess").

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use crate::{AcpError, Client, Event};

/// How an agent is started.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentCommand {
    /// The program.
    pub program: PathBuf,
    /// Its arguments.
    pub args: Vec<String>,
    /// Environment variables to set for it, each a name and a value.
    pub env: Vec<(String, String)>,
}

/// A running agent: the protocol's client on its standard input, and the lines of its
/// standard output as they come. Dropping it ends the process.
#[derive(Debug)]
pub struct Agent {
    client: Client<ChildStdin>,
    /// The lines the agent wrote, until its output closes.
    lines: Option<Receiver<String>>,
    child: Child,
}

impl Agent {
    /// Starts `command` in the directory `cwd`. `wake` is called, off the caller's thread,
    /// whenever a line has come or the agent's output has closed, so a window can draw.
    ///
    /// # Errors
    ///
    /// [`AcpError::Spawn`] when the program cannot be started.
    pub fn spawn(
        command: &AgentCommand,
        cwd: &Path,
        wake: impl Fn() + Send + 'static,
    ) -> Result<Self, AcpError> {
        let spawn_error = |error| AcpError::Spawn(command.program.clone(), error);
        let mut child = Command::new(&command.program)
            .args(&command.args)
            .envs(command.env.iter().map(|(name, value)| (name, value)))
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(spawn_error)?;
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err(spawn_error(std::io::Error::other("no pipe to the agent")));
        };
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    return;
                }
                wake();
            }
            drop(sender);
            wake();
        });
        // What the agent says of itself, which is not the protocol's, goes to the log.
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                tracing::debug!(%line, "agent");
            }
        });
        Ok(Self {
            client: Client::new(stdin),
            lines: Some(lines),
            child,
        })
    }

    /// The protocol's client, to send the agent requests and answers.
    pub const fn client(&mut self) -> &mut Client<ChildStdin> {
        &mut self.client
    }

    /// What the agent's lines since the last call mean, in order; [`Event::Exited`] comes
    /// last, once, when its output has closed. A line that is not a message is logged and
    /// passed over. It never waits.
    pub fn poll(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        while let Some(lines) = &self.lines {
            match lines.try_recv() {
                Ok(line) => self.read(&line, &mut events),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => self.exited(&mut events),
            }
        }
        events
    }

    /// As [`Self::poll`], after waiting for the agent's next line or its end; nothing once
    /// it has ended.
    pub fn wait(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        if let Some(lines) = &self.lines {
            match lines.recv() {
                Ok(line) => self.read(&line, &mut events),
                Err(_closed) => self.exited(&mut events),
            }
        }
        events.extend(self.poll());
        events
    }

    /// Adds what `line` means to `events`.
    fn read(&mut self, line: &str, events: &mut Vec<Event>) {
        match self.client.handle(line) {
            Ok(more) => events.extend(more),
            Err(error) => tracing::warn!(%error, "agent line not read"),
        }
    }

    /// The agent's output has closed: no more lines are read.
    fn exited(&mut self, events: &mut Vec<Event>) {
        self.lines = None;
        events.push(Event::Exited);
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        if let Err(error) = self.child.kill() {
            tracing::debug!(%error, "agent not killed");
        }
        // Reaped, so no zombie is left behind.
        if let Err(error) = self.child.wait() {
            tracing::debug!(%error, "agent not waited for");
        }
    }
}
