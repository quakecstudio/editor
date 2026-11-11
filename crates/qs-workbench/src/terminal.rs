// SPDX-License-Identifier: GPL-3.0-or-later

//! The terminal and project tasks (P4-36, UI-20): shells and tasks run in the project
//! directory, each a tab whose output is kept and whose exit status is shown, and the commands
//! that list and run the project's tasks and open, write to and close a tab.
//!
//! The model is Zed's and VS Code's: tasks are named command lines in a project file
//! (`quakec.toml`'s `[[task]]` tables, PRJ-2.10), each run by the user's shell in its own
//! terminal tab (PRJ-2.11), picked from the palette. A shell tab reads lines from its input
//! box: the processes run on pipes, not a pseudo-terminal, so a program that draws on the
//! screen (an editor, a pager) is not supported; their output is kept as text, without
//! escape sequences.

use std::ffi::OsString;
use std::fmt;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command as Process, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use qc_project::{ManifestError, PRJ_MANIFEST, Task};
use qs_commands::{Command, Outcome, RegisterError, Registry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::HasWorkbench;

/// The id of the command that lists the project's tasks.
pub const TASK_LIST: &str = "task.list";
/// The id of the command that runs a task in a new terminal tab.
pub const TASK_RUN: &str = "task.run";
/// The id of the command that opens a shell in a new terminal tab.
pub const TERMINAL_NEW: &str = "terminal.new";
/// The id of the command that writes a line to a terminal tab.
pub const TERMINAL_SEND: &str = "terminal.send";
/// The id of the command that takes the tabs' new output and exits.
pub const TERMINAL_POLL: &str = "terminal.poll";
/// The id of the command that closes a terminal tab, ending its process.
pub const TERMINAL_CLOSE: &str = "terminal.close";

/// The most output a tab keeps, in bytes; older lines are dropped past it.
pub const KEPT_OUTPUT: usize = 1 << 20;

/// How a tab's process ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Exit {
    /// It exited with this status.
    Code(i32),
    /// A signal ended it, or it was killed.
    Killed,
}

impl Exit {
    /// Whether it exited with status 0.
    #[must_use]
    pub const fn succeeded(self) -> bool {
        matches!(self, Self::Code(0))
    }
}

/// What a tab runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Running {
    /// The user's shell, reading lines from the input box.
    Shell,
    /// A task of the project (PRJ-2.10).
    Task {
        /// The task's name.
        name: String,
        /// Its command line.
        command: String,
    },
}

/// The user's shell and the flag that has it run one command line (PRJ-2.11).
fn shell() -> (OsString, &'static str) {
    if cfg!(windows) {
        let comspec = std::env::var_os("COMSPEC").filter(|s| !s.is_empty());
        (comspec.unwrap_or_else(|| "cmd.exe".into()), "/C")
    } else {
        let shell = std::env::var_os("SHELL").filter(|s| !s.is_empty());
        (shell.unwrap_or_else(|| "/bin/sh".into()), "-c")
    }
}

/// One terminal tab: its process, the output it has written and how it ended.
#[derive(Debug)]
pub struct Terminal {
    running: Running,
    dir: PathBuf,
    child: Child,
    stdin: Option<ChildStdin>,
    output: Receiver<Vec<u8>>,
    text: String,
    exit: Option<Exit>,
}

impl Terminal {
    /// Starts `running` in `dir`: the shell reading its input, or a task's command line run by
    /// the shell (PRJ-2.11), with stdout and stderr both kept.
    ///
    /// # Errors
    ///
    /// The shell could not be started.
    pub fn start(running: Running, dir: &Path) -> io::Result<Self> {
        let (program, flag) = shell();
        let mut process = Process::new(&program);
        process
            .current_dir(dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        match &running {
            Running::Shell => {
                process.stdin(Stdio::piped());
            }
            Running::Task { command, .. } => {
                process.arg(flag).arg(command).stdin(Stdio::null());
            }
        }
        let mut child = process.spawn()?;
        let (sender, output) = mpsc::channel();
        if let Some(out) = child.stdout.take() {
            read_into(out, sender.clone());
        }
        if let Some(err) = child.stderr.take() {
            read_into(err, sender);
        }
        let stdin = child.stdin.take();
        let mut text = String::new();
        if let Running::Task { command, .. } = &running {
            text.push_str("> ");
            text.push_str(command);
            text.push('\n');
        }
        Ok(Self {
            running,
            dir: dir.to_path_buf(),
            child,
            stdin,
            output,
            text,
            exit: None,
        })
    }

    /// What it runs.
    #[must_use]
    pub const fn running(&self) -> &Running {
        &self.running
    }

    /// The tab's title: the task's name, or the shell's directory's name.
    #[must_use]
    pub fn title(&self) -> String {
        match &self.running {
            Running::Task { name, .. } => name.clone(),
            Running::Shell => self.dir.file_name().map_or_else(
                || self.dir.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            ),
        }
    }

    /// The output so far, the newest last.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// How the process ended, once it has.
    #[must_use]
    pub const fn exit(&self) -> Option<Exit> {
        self.exit
    }

    /// Takes the output written since the last call, and notes the exit once the process has
    /// ended and its output is all read; true when anything changed. It never blocks.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        let mut closed = false;
        loop {
            match self.output.try_recv() {
                Ok(bytes) => {
                    self.push(&bytes);
                    changed = true;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    closed = true;
                    break;
                }
            }
        }
        if self.exit.is_none()
            && closed
            && let Ok(Some(status)) = self.child.try_wait()
        {
            self.exit = Some(status.code().map_or(Exit::Killed, Exit::Code));
            changed = true;
        }
        changed
    }

    /// Waits for the process to end and its output to be read, then gives how it ended.
    ///
    /// # Errors
    ///
    /// Waiting for the process failed.
    pub fn wait(&mut self) -> io::Result<Exit> {
        self.stdin = None;
        let status = self.child.wait()?;
        while let Ok(bytes) = self.output.recv() {
            self.push(&bytes);
        }
        let exit = status.code().map_or(Exit::Killed, Exit::Code);
        self.exit = Some(exit);
        Ok(exit)
    }

    /// Writes `line` and a line break to the shell's input, echoed in the output.
    ///
    /// # Errors
    ///
    /// The tab runs a task, whose input is closed, or the process has ended.
    pub fn send(&mut self, line: &str) -> io::Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "the input is closed"))?;
        stdin.write_all(line.as_bytes())?;
        stdin.write_all(b"\n")?;
        stdin.flush()?;
        self.push(format!("$ {line}\n").as_bytes());
        Ok(())
    }

    /// Adds `bytes` to the output, as UTF-8 with any bad sequence replaced, dropping the
    /// oldest lines past [`KEPT_OUTPUT`].
    fn push(&mut self, bytes: &[u8]) {
        self.text.push_str(&String::from_utf8_lossy(bytes));
        if self.text.len() > KEPT_OUTPUT {
            let over = self.text.len().saturating_sub(KEPT_OUTPUT);
            let cut = self
                .text
                .get(over..)
                .and_then(|rest| rest.find('\n'))
                .map_or(over, |at| over.saturating_add(at).saturating_add(1));
            let cut = (cut..=self.text.len())
                .find(|at| self.text.is_char_boundary(*at))
                .unwrap_or(self.text.len());
            self.text.drain(..cut);
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if self.exit.is_none() {
            // A process that already ended cannot be killed; there is nothing more to do.
            let _ended = self.child.kill();
            let _reaped = self.child.wait();
        }
    }
}

/// Sends what `from` writes to `to`, in chunks, until it closes.
fn read_into(mut from: impl Read + Send + 'static, to: Sender<Vec<u8>>) {
    thread::spawn(move || {
        let mut buffer = vec![0_u8; 8192];
        loop {
            match from.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    let chunk = buffer.get(..read).unwrap_or_default().to_vec();
                    if to.send(chunk).is_err() {
                        break;
                    }
                }
            }
        }
    });
}

/// The terminal tabs, in the order opened.
#[derive(Debug, Default)]
pub struct Terminals {
    tabs: Vec<Terminal>,
}

impl Terminals {
    /// The tabs.
    #[must_use]
    pub fn tabs(&self) -> &[Terminal] {
        &self.tabs
    }

    /// The tab at `index`.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut Terminal> {
        self.tabs.get_mut(index)
    }

    /// Polls every tab ([`Terminal::poll`]); true when any changed.
    pub fn poll(&mut self) -> bool {
        self.tabs
            .iter_mut()
            .fold(false, |changed, tab| tab.poll() | changed)
    }

    /// Adds `tab` and gives its index.
    fn push(&mut self, tab: Terminal) -> usize {
        self.tabs.push(tab);
        self.tabs.len().saturating_sub(1)
    }
}

/// The tasks of the project at `root`: its `quakec.toml`'s (PRJ-2.10), none without one.
///
/// # Errors
///
/// The file cannot be read or is not a valid project file.
pub fn tasks(root: &Path) -> Result<Vec<Task>, TerminalError> {
    let path = root.join(PRJ_MANIFEST);
    match std::fs::read_to_string(&path) {
        Ok(text) => qc_project::tasks(&text).map_err(TerminalError::Manifest),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(TerminalError::Io(error.to_string())),
    }
}

/// Why a terminal command failed.
#[derive(Debug)]
pub enum TerminalError {
    /// No project is open.
    NoProject,
    /// The project file is not valid.
    Manifest(ManifestError),
    /// The project has no task of this name.
    NoTask(String),
    /// There is no tab at this index.
    NoTab(usize),
    /// Starting the shell, or writing to it, failed.
    Io(String),
}

impl fmt::Display for TerminalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoProject => f.write_str("no project is open"),
            Self::Manifest(error) => write!(f, "{PRJ_MANIFEST}: {error}"),
            Self::NoTask(name) => write!(f, "the project has no task named {name:?}"),
            Self::NoTab(index) => write!(f, "there is no terminal tab {index}"),
            Self::Io(error) => f.write_str(error),
        }
    }
}

impl std::error::Error for TerminalError {}

/// A task, as `task.list` lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TaskInfo {
    /// Its name.
    pub name: String,
    /// Its command line.
    pub command: String,
}

/// No parameters.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoParams {}

/// The parameters of `task.run`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunTaskParams {
    /// The task's name, as `task.list` gives it.
    pub name: String,
}

/// The parameters of `terminal.send`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SendParams {
    /// The tab, from 0 in the order opened.
    pub tab: usize,
    /// The line to write.
    pub line: String,
}

/// The parameters of `terminal.close`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CloseParams {
    /// The tab, from 0 in the order opened.
    pub tab: usize,
}

/// What `task.run` and `terminal.new` answer: the new tab.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Opened {
    /// Its index, from 0 in the order opened.
    pub tab: usize,
    /// Its title.
    pub title: String,
}

/// The open project's directory.
fn root<C: HasWorkbench>(context: &mut C) -> Result<PathBuf, TerminalError> {
    context
        .workbench()
        .root()
        .map(Path::to_path_buf)
        .ok_or(TerminalError::NoProject)
}

/// Starts `running` in the open project's directory in a new tab.
fn open<C: HasWorkbench>(context: &mut C, running: Running) -> Result<Opened, TerminalError> {
    let dir = root(context)?;
    let tab = Terminal::start(running, &dir).map_err(|e| TerminalError::Io(e.to_string()))?;
    let title = tab.title();
    let tab = context.workbench().terminals_mut().push(tab);
    Ok(Opened { tab, title })
}

/// `task.list`.
struct ListTasks;

impl<C: HasWorkbench + 'static> Command<C> for ListTasks {
    const ID: &'static str = TASK_LIST;
    const SUMMARY: &'static str =
        "Lists the open project's tasks: the named command lines of its quakec.toml.";
    type Params = NoParams;
    type Output = Vec<TaskInfo>;
    type Error = TerminalError;

    fn run(&self, context: &mut C, _: NoParams) -> Result<Outcome<Vec<TaskInfo>, C>, Self::Error> {
        let listed = tasks(&root(context)?)?
            .into_iter()
            .map(|task| TaskInfo {
                name: task.name,
                command: task.command,
            })
            .collect();
        Ok(Outcome::outside_history(listed))
    }
}

/// `task.run`.
struct RunTask;

impl<C: HasWorkbench + 'static> Command<C> for RunTask {
    const ID: &'static str = TASK_RUN;
    const SUMMARY: &'static str = "Runs one of the open project's tasks in a new terminal tab, \
        by the user's shell in the project directory.";
    type Params = RunTaskParams;
    type Output = Opened;
    type Error = TerminalError;

    fn run(
        &self,
        context: &mut C,
        params: RunTaskParams,
    ) -> Result<Outcome<Opened, C>, Self::Error> {
        let task = tasks(&root(context)?)?
            .into_iter()
            .find(|task| task.name == params.name)
            .ok_or(TerminalError::NoTask(params.name))?;
        let running = Running::Task {
            name: task.name,
            command: task.command,
        };
        open(context, running).map(Outcome::outside_history)
    }
}

/// `terminal.new`.
struct NewTerminal;

impl<C: HasWorkbench + 'static> Command<C> for NewTerminal {
    const ID: &'static str = TERMINAL_NEW;
    const SUMMARY: &'static str =
        "Opens the user's shell in the project directory in a new terminal tab.";
    type Params = NoParams;
    type Output = Opened;
    type Error = TerminalError;

    fn run(&self, context: &mut C, _: NoParams) -> Result<Outcome<Opened, C>, Self::Error> {
        open(context, Running::Shell).map(Outcome::outside_history)
    }
}

/// `terminal.send`.
struct SendLine;

impl<C: HasWorkbench + 'static> Command<C> for SendLine {
    const ID: &'static str = TERMINAL_SEND;
    const SUMMARY: &'static str = "Writes a line to a terminal tab's shell.";
    type Params = SendParams;
    type Output = ();
    type Error = TerminalError;

    fn run(&self, context: &mut C, params: SendParams) -> Result<Outcome<(), C>, Self::Error> {
        let tab = context
            .workbench()
            .terminals_mut()
            .get_mut(params.tab)
            .ok_or(TerminalError::NoTab(params.tab))?;
        tab.send(&params.line)
            .map_err(|e| TerminalError::Io(e.to_string()))?;
        Ok(Outcome::outside_history(()))
    }
}

/// `terminal.poll`.
struct PollTerminals;

impl<C: HasWorkbench + 'static> Command<C> for PollTerminals {
    const ID: &'static str = TERMINAL_POLL;
    const SUMMARY: &'static str = "Takes what the terminal tabs have written and how their \
        processes ended, without waiting; true when any tab changed.";
    type Params = NoParams;
    type Output = bool;
    type Error = TerminalError;

    fn run(&self, context: &mut C, _: NoParams) -> Result<Outcome<bool, C>, Self::Error> {
        Ok(Outcome::outside_history(
            context.workbench().terminals_mut().poll(),
        ))
    }
}

/// `terminal.close`.
struct CloseTerminal;

impl<C: HasWorkbench + 'static> Command<C> for CloseTerminal {
    const ID: &'static str = TERMINAL_CLOSE;
    const SUMMARY: &'static str = "Closes a terminal tab, ending its process.";
    type Params = CloseParams;
    type Output = ();
    type Error = TerminalError;

    fn run(&self, context: &mut C, params: CloseParams) -> Result<Outcome<(), C>, Self::Error> {
        let terminals = context.workbench().terminals_mut();
        if params.tab >= terminals.tabs.len() {
            return Err(TerminalError::NoTab(params.tab));
        }
        drop(terminals.tabs.remove(params.tab));
        Ok(Outcome::outside_history(()))
    }
}

/// Registers the task and terminal commands.
///
/// # Errors
///
/// [`RegisterError`] when an id is already registered.
pub fn register_terminal<C: HasWorkbench + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(ListTasks)?;
    registry.register(RunTask)?;
    registry.register(NewTerminal)?;
    registry.register(SendLine)?;
    registry.register(PollTerminals)?;
    registry.register(CloseTerminal)
}

#[cfg(test)]
mod tests;
