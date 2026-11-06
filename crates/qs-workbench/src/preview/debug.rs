// SPDX-License-Identifier: GPL-3.0-or-later

//! The debugger's state (P3-16): the breakpoints the user set, kept while no game runs and set
//! in each game that starts; where the game stopped, its call stack and the watches' values,
//! read from the host's `breakpoint/hit`, `qc/error` and `state/changed` events and its
//! `inspect stack` and `evaluate` answers (BRIDGE-3, BRIDGE-4).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A breakpoint: a line of a file, relative to the project's root, and the id the running host
/// gave it, if it is set there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Breakpoint {
    /// The file, relative to the project's root.
    pub file: PathBuf,
    /// The line, from 1.
    pub line: u32,
    /// The host's id for it, while a game runs and took it.
    pub id: Option<u64>,
    /// The function the host found at the line.
    pub function: Option<String>,
}

/// Where a run is: a function, and its file and line when the program has a `.lno`
/// (BRIDGE-3.4); an engine frame names its function alone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    /// The function.
    pub function: String,
    /// The file, relative to the project's root.
    pub file: Option<PathBuf>,
    /// The line, from 1.
    pub line: Option<u32>,
    /// Whether it is the engine's frame, not QuakeC's.
    pub engine: bool,
}

impl Location {
    /// The location a bridge object gives, `None` without a function.
    #[must_use]
    pub fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            function: value.get("function")?.as_str()?.to_owned(),
            file: value.get("file").and_then(Value::as_str).map(PathBuf::from),
            line: value
                .get("line")
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok()),
            engine: value
                .get("engine")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
    }
}

/// A watch: an expression and its value when the game last stopped.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Watch {
    /// The expression, such as `self.owner.health`.
    pub expression: String,
    /// Its value as the Debugger tab writes it, or why it has none.
    pub value: Option<String>,
}

/// Why the game is stopped in a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StopReason {
    /// A breakpoint was reached.
    Breakpoint,
    /// A step ended.
    Step,
    /// The VM stopped with a run error.
    Error,
}

impl StopReason {
    /// The reason `state/changed` names, when it stops the game in a frame (BRIDGE-3.3).
    #[must_use]
    pub fn of(reason: &str) -> Option<Self> {
        match reason {
            "breakpoint" => Some(Self::Breakpoint),
            "step" => Some(Self::Step),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

/// The debugger's state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Debugger {
    breakpoints: Vec<Breakpoint>,
    stopped: Option<(StopReason, Location)>,
    stack: Vec<Location>,
    watches: Vec<Watch>,
    /// The last run error's message.
    error: Option<String>,
}

impl Debugger {
    /// No breakpoint, nothing stopped and no watch.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            breakpoints: Vec::new(),
            stopped: None,
            stack: Vec::new(),
            watches: Vec::new(),
            error: None,
        }
    }

    /// The breakpoints, in the order they were set.
    #[must_use]
    pub fn breakpoints(&self) -> &[Breakpoint] {
        &self.breakpoints
    }

    /// The lines of `file` that have a breakpoint.
    pub fn lines_of<'a>(&'a self, file: &'a Path) -> impl Iterator<Item = u32> + 'a {
        self.breakpoints
            .iter()
            .filter(move |b| b.file == file)
            .map(|b| b.line)
    }

    /// Where the game is stopped in a frame, and why.
    #[must_use]
    pub const fn stopped(&self) -> Option<&(StopReason, Location)> {
        self.stopped.as_ref()
    }

    /// The call stack while stopped, the innermost frame first.
    #[must_use]
    pub fn stack(&self) -> &[Location] {
        &self.stack
    }

    /// The watches.
    #[must_use]
    pub fn watches(&self) -> &[Watch] {
        &self.watches
    }

    /// The last run error's message.
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Adds the breakpoint at `line` of `file`, or takes it away when there is one; the one
    /// taken away, whose host id is to be cleared, or `None` when one was added.
    pub fn toggle(&mut self, file: &Path, line: u32) -> Option<Breakpoint> {
        if let Some(at) = self
            .breakpoints
            .iter()
            .position(|b| b.file == file && b.line == line)
        {
            return Some(self.breakpoints.remove(at));
        }
        self.breakpoints.push(Breakpoint {
            file: file.to_path_buf(),
            line,
            id: None,
            function: None,
        });
        None
    }

    /// The breakpoint at `line` of `file`, to record what the host answered.
    pub fn breakpoint_mut(&mut self, file: &Path, line: u32) -> Option<&mut Breakpoint> {
        self.breakpoints
            .iter_mut()
            .find(|b| b.file == file && b.line == line)
    }

    /// Forgets every host id, as when the game stops.
    pub fn unset_all(&mut self) {
        for breakpoint in &mut self.breakpoints {
            breakpoint.id = None;
        }
        self.resumed();
    }

    /// Adds a watch.
    pub fn watch(&mut self, expression: &str) {
        let expression = expression.trim();
        if expression.is_empty() || self.watches.iter().any(|w| w.expression == expression) {
            return;
        }
        self.watches.push(Watch {
            expression: expression.to_owned(),
            value: None,
        });
    }

    /// Takes away watch `index`.
    pub fn unwatch(&mut self, index: usize) {
        if index < self.watches.len() {
            self.watches.remove(index);
        }
    }

    /// The watches, to set their values.
    pub fn watches_mut(&mut self) -> &mut [Watch] {
        &mut self.watches
    }

    /// The game stopped in a frame at `location`, for `reason`.
    pub fn stop(&mut self, reason: StopReason, location: Location) {
        self.stopped = Some((reason, location));
    }

    /// The call stack from an `inspect stack` answer (BRIDGE-4.9).
    pub fn set_stack(&mut self, answer: &Value) {
        self.stack = answer
            .get("stack")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Location::from_value)
            .collect();
    }

    /// A run error's message (`qc/error`, BRIDGE-3.1).
    pub fn failed(&mut self, message: &str) {
        self.error = Some(message.to_owned());
    }

    /// The game runs again: nothing is stopped and the stack is gone.
    pub fn resumed(&mut self) {
        self.stopped = None;
        self.stack.clear();
    }
}

#[cfg(test)]
mod tests;
