// SPDX-License-Identifier: GPL-3.0-or-later
//! The benchmark harness both prototypes share: command-line arguments, the per-frame driver
//! that replays the script, and the startup timings.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::document::Document;
use crate::project::{self, Project};
use crate::script::{self, Action, Phase};
use crate::stats::{self, Recorder, Sample};

/// Command-line arguments: `<project-dir> [--lines N] [--bench OUT.csv] [--no-viewport]`.
#[derive(Debug, PartialEq, Eq)]
pub struct Args {
    pub project: PathBuf,
    pub lines: usize,
    pub bench: Option<PathBuf>,
    /// Draw the 3D viewport; `--no-viewport` measures the editor alone.
    pub viewport: bool,
}

/// The default buffer size: the acceptance test's 50,000-line project.
pub const DEFAULT_LINES: usize = 50_000;

impl Args {
    /// Parses arguments (without the program name).
    ///
    /// # Errors
    /// Returns a usage message for anything it does not understand.
    pub fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let usage = "usage: <project-dir> [--lines N] [--bench OUT.csv] [--no-viewport]".to_owned();
        let mut project = None;
        let mut lines = DEFAULT_LINES;
        let mut bench = None;
        let mut viewport = true;
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--lines" => {
                    lines = args
                        .next()
                        .and_then(|n| n.parse().ok())
                        .ok_or_else(|| usage.clone())?;
                }
                "--no-viewport" => viewport = false,
                "--bench" => bench = Some(PathBuf::from(args.next().ok_or_else(|| usage.clone())?)),
                other if other.starts_with('-') => return Err(usage),
                other => project = Some(PathBuf::from(other)),
            }
        }
        Ok(Self {
            project: project.ok_or(usage)?,
            lines,
            bench,
            viewport,
        })
    }

    /// The label a run is recorded under: the framework, plus `-noviewport` when it is off.
    #[must_use]
    pub fn label(&self, framework: &str) -> String {
        if self.viewport {
            framework.to_owned()
        } else {
            format!("{framework}-noviewport")
        }
    }
}

/// A project loaded into a document, with how long that took.
#[derive(Debug)]
pub struct Opened {
    pub project: Project,
    pub doc: Document,
    pub load: Duration,
}

/// Loads the project and builds the document (symbols and lexer states included).
///
/// # Errors
/// Fails when the project cannot be read.
pub fn open(args: &Args) -> std::io::Result<Opened> {
    let start = Instant::now();
    let project = project::load(&args.project, args.lines)?;
    let mut doc = Document::new(&project.text);
    doc.goto_line(script::START_LINE, script::CONTEXT_ROWS);
    Ok(Opened {
        project,
        doc,
        load: start.elapsed(),
    })
}

/// Replays the script one action per frame and records each frame.
#[derive(Debug)]
pub struct Driver {
    steps: Vec<(Phase, Action)>,
    next: usize,
    last_frame: Option<Instant>,
    pub recorder: Recorder,
}

/// What the driver did this frame, to be completed with the frame's CPU time.
#[derive(Clone, Copy, Debug)]
pub struct Pending {
    pub phase: Phase,
    pub sample: Sample,
}

impl Default for Driver {
    fn default() -> Self {
        Self::new()
    }
}

impl Driver {
    #[must_use]
    pub fn new() -> Self {
        Self {
            steps: script::script(),
            next: 0,
            last_frame: None,
            recorder: Recorder::default(),
        }
    }

    /// Whether every step has been applied.
    #[must_use]
    pub fn finished(&self) -> bool {
        self.next >= self.steps.len()
    }

    #[must_use]
    pub fn progress(&self) -> (usize, usize) {
        (self.next, self.steps.len())
    }

    /// Applies the next action at the start of a frame. Returns `None` once the script is done.
    pub fn begin_frame(&mut self, doc: &mut Document, visible_rows: usize) -> Option<Pending> {
        let now = Instant::now();
        let interval = self
            .last_frame
            .map(|t| now.duration_since(t))
            .unwrap_or_default();
        self.last_frame = Some(now);
        let (phase, action) = self.steps.get(self.next)?;
        let phase = *phase;
        let start = Instant::now();
        script::apply(doc, action, visible_rows);
        let action_ns = nanos(start.elapsed());
        let sample = Sample {
            frame: self.next,
            frame_ns: 0,
            action_ns,
            interval_ns: nanos(interval),
            draw_ns: 0,
            present_ns: 0,
        };
        self.next = self.next.saturating_add(1);
        Some(Pending { phase, sample })
    }

    /// Records a frame once its CPU time is known.
    pub fn record(&mut self, pending: Pending, frame_ns: u64) {
        self.record_split(pending, frame_ns, 0, 0);
    }

    /// Records a frame whose framework reports draw and present times separately.
    pub fn record_split(&mut self, pending: Pending, frame_ns: u64, draw_ns: u64, present_ns: u64) {
        let mut sample = pending.sample;
        sample.frame_ns = frame_ns;
        sample.draw_ns = draw_ns;
        sample.present_ns = present_ns;
        self.recorder.push(pending.phase, sample);
    }

    /// Writes the CSV and returns the summary table.
    ///
    /// # Errors
    /// Fails when the CSV cannot be written.
    pub fn write(&self, out: &std::path::Path, framework: &str) -> std::io::Result<String> {
        self.recorder.write(out, framework, &stats::host())
    }
}

/// A duration in whole nanoseconds, saturating.
#[must_use]
pub fn nanos(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Result<Args, String> {
        Args::parse(list.iter().map(|s| (*s).to_owned()))
    }

    #[test]
    fn parses_arguments() {
        assert_eq!(
            args(&["dir", "--lines", "10", "--bench", "o.csv"]),
            Ok(Args {
                project: "dir".into(),
                lines: 10,
                bench: Some("o.csv".into()),
                viewport: true,
            })
        );
        let off = args(&["dir", "--no-viewport"]);
        assert_eq!(off.as_ref().map(|a| a.viewport), Ok(false));
        assert_eq!(
            off.map(|a| a.label("egui")),
            Ok("egui-noviewport".to_owned())
        );
        assert_eq!(args(&["dir"]).map(|a| a.lines), Ok(DEFAULT_LINES));
        assert!(args(&[]).is_err());
        assert!(args(&["dir", "--lines"]).is_err());
        assert!(args(&["dir", "--lines", "x"]).is_err());
        assert!(args(&["dir", "--what"]).is_err());
    }

    #[test]
    fn the_driver_records_one_sample_per_step() {
        let mut doc = Document::new("a\nb\n");
        let mut driver = Driver::new();
        let (_, total) = driver.progress();
        while let Some(p) = driver.begin_frame(&mut doc, 10) {
            driver.record(p, 1);
        }
        assert!(driver.finished());
        assert_eq!(driver.recorder.len(), total);
    }
}
