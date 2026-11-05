// SPDX-License-Identifier: GPL-3.0-or-later

//! Debugging a world's QuakeC (WORLD-9): breakpoints at statements, the rules that step from
//! where a run stopped, and where a statement is in the source. The [`Debugger`] is the VM's
//! observer during a frame and stops the run before a statement (VM-20.1); a host replays the
//! frame from a snapshot of the world taken before it, with a rule that skips the statements
//! already run, to step or to go on (WORLD-9.2).

use std::collections::BTreeSet;

use qc_progs::Progs;
use qc_vm::{Machine, Observer, RunError};

/// How far a step goes from where the run stopped (BRIDGE-4.1, WORLD-9.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepKind {
    /// One statement.
    Statement,
    /// To the next line, into a function the line calls.
    In,
    /// To the next line of this function or its callers, over the calls.
    Over,
    /// To the caller.
    Out,
}

/// When a frame's run stops (WORLD-9.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rule {
    /// At a breakpoint, after the first `skip` statements.
    Breakpoints { skip: u64 },
    /// As `kind` says from the statement `skip` stopped at, `depth` deep on `line` in the
    /// frame's `run`th run, or at a breakpoint on the way for every kind but a single
    /// statement.
    Step {
        kind: StepKind,
        skip: u64,
        depth: usize,
        line: Option<i32>,
        run: u64,
    },
    /// Never: the frame runs to its end.
    Finish,
}

/// Where a run stopped (WORLD-9.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stop {
    /// The statements asked about in the frame up to and including this one, the replay's
    /// `skip` (WORLD-9.2).
    pub count: u64,
    /// The function it is in.
    pub function: i32,
    /// The statement that would run next.
    pub statement: i32,
    /// The call depth.
    pub depth: usize,
    /// Which of the frame's runs, counted from 1, the engine started (WORLD-9.3).
    pub run: u64,
    /// The breakpoint it stopped at, when it did.
    pub breakpoint: Option<i32>,
}

/// The VM's observer while a host debugs: the breakpoints, the rule of this frame's run and
/// where it stopped (WORLD-9).
#[derive(Clone, Debug)]
pub struct Debugger {
    breakpoints: BTreeSet<i32>,
    lines: Vec<i32>,
    rule: Rule,
    count: u64,
    runs: u64,
    stop: Option<Stop>,
}

impl Debugger {
    /// A debugger with no breakpoints over a progs whose `.lno` gives `lines`, the source line
    /// of each statement; empty without one, each statement then its own line.
    #[must_use]
    pub fn new(lines: Vec<i32>) -> Self {
        Self {
            breakpoints: BTreeSet::new(),
            lines,
            rule: Rule::Breakpoints { skip: 0 },
            count: 0,
            runs: 0,
            stop: None,
        }
    }

    /// Sets or clears a breakpoint before `statement`.
    pub fn set_breakpoint(&mut self, statement: i32, on: bool) {
        if on {
            self.breakpoints.insert(statement);
        } else {
            self.breakpoints.remove(&statement);
        }
    }

    /// Whether the next frame may stop, at a breakpoint or a step: a host snapshots a frame
    /// only then (WORLD-9.2).
    #[must_use]
    pub fn may_stop(&self) -> bool {
        match self.rule {
            Rule::Step { .. } => true,
            Rule::Breakpoints { .. } => !self.breakpoints.is_empty(),
            Rule::Finish => false,
        }
    }

    /// The next frame stops at a breakpoint after the first `skip` statements: 0 for a new
    /// frame, a stop's count to go on from it (WORLD-9.2).
    pub const fn run_to_breakpoint(&mut self, skip: u64) {
        self.rule = Rule::Breakpoints { skip };
    }

    /// The next frame stops as `kind` steps from `from` (WORLD-9.3).
    pub fn step_from(&mut self, kind: StepKind, from: &Stop) {
        self.rule = Rule::Step {
            kind,
            skip: from.count,
            depth: from.depth,
            line: self.line_of(from.statement),
            run: from.run,
        };
    }

    /// The next frame stops at its first statement, the start of a step from between two
    /// frames.
    pub const fn stop_at_first(&mut self) {
        self.rule = Rule::Step {
            kind: StepKind::Statement,
            skip: 0,
            depth: 0,
            line: None,
            run: 0,
        };
    }

    /// The next frame runs to its end.
    pub const fn finish(&mut self) {
        self.rule = Rule::Finish;
    }

    /// Where the last run stopped, `None` when it ran to its end.
    #[must_use]
    pub const fn stopped(&self) -> Option<Stop> {
        self.stop
    }

    /// A frame starts: its statements are counted from 0 (WORLD-9.2).
    pub(crate) const fn begin_frame(&mut self) {
        self.count = 0;
        self.runs = 0;
        self.stop = None;
    }

    /// The engine starts a run of `depth` 0, not one a builtin nests (VM-18).
    pub(crate) fn begin_run(&mut self, depth: usize) {
        if depth == 0 {
            self.runs = self.runs.saturating_add(1);
        }
    }

    /// The source line of `statement`, `None` without a `.lno`.
    fn line_of(&self, statement: i32) -> Option<i32> {
        usize::try_from(statement)
            .ok()
            .and_then(|s| self.lines.get(s))
            .copied()
    }

    /// WORLD-9.3: whether the rule stops before `statement`, `depth` deep.
    fn stops(&self, statement: i32, depth: usize) -> bool {
        let breakpoint = self.breakpoints.contains(&statement);
        match self.rule {
            Rule::Finish => false,
            Rule::Breakpoints { skip } => self.count > skip && breakpoint,
            Rule::Step {
                kind,
                skip,
                depth: from,
                line,
                run,
            } => {
                if self.count <= skip {
                    return false;
                }
                // Without a .lno every statement is a line of its own.
                let other_line = line.is_none() || self.line_of(statement) != line;
                // Over and out stay in the run they started in: past its end the engine runs
                // other functions, which are not this one's callers.
                let same_run = self.runs == run;
                match kind {
                    StepKind::Statement => true,
                    StepKind::In => breakpoint || depth != from || other_line,
                    StepKind::Over => {
                        breakpoint || (same_run && (depth < from || (depth == from && other_line)))
                    }
                    StepKind::Out => breakpoint || (same_run && depth < from),
                }
            }
        }
    }
}

impl Observer for Debugger {
    fn run(&mut self, machine: &Machine, _function: i32) {
        self.begin_run(machine.depth());
    }

    fn step(&mut self, _m: &Machine, _f: i32, _s: i32, _o: u16, _b: Option<u32>) {}

    fn exit(&mut self, _machine: &Machine) {}

    fn error(&mut self, _machine: &Machine, _error: &RunError) {}

    fn stop_before(&mut self, machine: &Machine, function: i32, statement: i32) -> bool {
        self.count = self.count.saturating_add(1);
        let depth = machine.depth();
        if !self.stops(statement, depth) {
            return false;
        }
        self.stop = Some(Stop {
            count: self.count,
            function,
            statement,
            depth,
            run: self.runs,
            breakpoint: self.breakpoints.contains(&statement).then_some(statement),
        });
        true
    }
}

/// Where a statement is in the source (BRIDGE-3.4): its function's name and, with a `.lno`,
/// the function's file and the statement's line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    /// The function's name.
    pub function: String,
    /// The file the function is in, and the line, when the progs has a `.lno`.
    pub source: Option<(String, i32)>,
    /// The statement's index.
    pub statement: i32,
}

/// The location of `statement` of `function` (BRIDGE-3.4, WORLD-9.4).
#[must_use]
pub fn locate(progs: &Progs<'_>, lines: &[i32], function: i32, statement: i32) -> Location {
    let text = |offset: i32| {
        progs
            .string(offset)
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
            .unwrap_or_default()
    };
    let record = usize::try_from(function)
        .ok()
        .and_then(|index| progs.functions().get(index));
    let name = record.map(|f| text(f.s_name)).unwrap_or_default();
    let line = usize::try_from(statement)
        .ok()
        .and_then(|s| lines.get(s))
        .copied();
    let source = record.zip(line).map(|(f, line)| (text(f.s_file), line));
    Location {
        function: name,
        source,
        statement,
    }
}

/// The statement a breakpoint at `line` of `file` stops before (BRIDGE-4.1, WORLD-9.4): the
/// first, in statement order, of a function whose file is `file` (or ends with `/` and it,
/// or is its last component) whose `.lno` line is `line`.
#[must_use]
pub fn resolve(progs: &Progs<'_>, lines: &[i32], file: &str, line: i32) -> Option<i32> {
    let functions: Vec<_> = progs.functions().iter().collect();
    let mut starts: Vec<i32> = functions
        .iter()
        .map(|f| f.first_statement)
        .filter(|&s| s > 0)
        .collect();
    starts.sort_unstable();
    // A Windows client's separators are the compiler's `/`.
    let file = file.replace('\\', "/");
    let file = file.as_str();
    let matches_file = |name: &[u8]| {
        let name = String::from_utf8_lossy(name).replace('\\', "/");
        name == file || file.ends_with(&format!("/{name}")) || name.ends_with(&format!("/{file}"))
    };
    let mut best: Option<i32> = None;
    for function in &functions {
        let first = function.first_statement;
        if first <= 0 || !progs.string(function.s_file).is_some_and(matches_file) {
            continue;
        }
        let end = starts
            .iter()
            .copied()
            .find(|&s| s > first)
            .unwrap_or_else(|| i32::try_from(lines.len()).unwrap_or(i32::MAX));
        let found = (first..end).find(|&s| {
            usize::try_from(s)
                .ok()
                .and_then(|i| lines.get(i))
                .is_some_and(|&l| l == line)
        });
        if let Some(found) = found {
            best = Some(best.map_or(found, |b| b.min(found)));
        }
    }
    best
}

/// The observer of a frame's runs (WORLD-9.1, PHYS-13.7): each statement counted against its
/// function, as id's `pr_xfunction->profile++` counts it, then the debugger asked, when there
/// is one.
pub(crate) struct Watch<'a> {
    pub(crate) debugger: Option<&'a mut Debugger>,
    pub(crate) profile: &'a mut Vec<u64>,
}

impl Observer for Watch<'_> {
    fn run(&mut self, machine: &Machine, _function: i32) {
        if let Some(debugger) = self.debugger.as_mut() {
            debugger.begin_run(machine.depth());
        }
    }

    fn step(&mut self, _m: &Machine, _f: i32, _s: i32, _o: u16, _b: Option<u32>) {}

    fn exit(&mut self, _machine: &Machine) {}

    fn error(&mut self, _machine: &Machine, _error: &RunError) {}

    fn stop_before(&mut self, machine: &Machine, function: i32, statement: i32) -> bool {
        if let Ok(index) = usize::try_from(function) {
            if self.profile.len() <= index {
                self.profile.resize(index.saturating_add(1), 0);
            }
            if let Some(count) = self.profile.get_mut(index) {
                *count = count.saturating_add(1);
            }
        }
        self.debugger
            .as_mut()
            .is_some_and(|debugger| debugger.stop_before(machine, function, statement))
    }
}
