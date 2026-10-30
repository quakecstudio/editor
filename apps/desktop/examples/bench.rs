// SPDX-License-Identifier: GPL-3.0-or-later

//! The code editor's budgets (P2-18, PRIN-5) on a 50,000-line project, in the app's own
//! window with the wgpu renderer:
//!
//! ```text
//! cargo run --release -p quakec-studio --example bench -- <out-dir> [host]
//! ```
//!
//! It writes a project of 25 files of 2,000 lines and a `defs.qc`, then measures:
//!
//! - **open**: from creating the app to the end of its first frame with the project open and
//!   the middle file in the editor, against PRIN-5's one second;
//! - **type**: each frame's CPU time (eframe's `Frame::info().cpu_usage`, as the P2-27 bench
//!   measures it) while typing one character a frame in the middle of that file, against one
//!   frame at 120 Hz (ADR-0003's bar);
//! - **check**: the frames of the 600 ms after typing stops, one of which sends the text to the
//!   project and checks it again, reported against the same bar.
//!
//! It writes `<host>-editor.md` into `<out-dir>` and exits 1 when the open or type budget is
//! missed. Keep the window on screen: an occluded window gets no frames on macOS.

#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "a benchmark binary that reports its summary"
)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use quakec_studio::App;
use serde_json::json;

/// One frame at 120 Hz, in nanoseconds: the pass bar of ADR-0003.
const FRAME_NS: u64 = 8_333_333;
/// PRIN-5's budget to open a project and show a file.
const OPEN: Duration = Duration::from_secs(1);
/// The project's files of QuakeC, and each one's lines.
const FILES: usize = 25;
const LINES_PER_FILE: usize = 2_000;
/// The file the editor shows, and the line typed at.
const MIDDLE_FILE: usize = 12;
const MIDDLE_LINE: usize = 1_000;
/// The text typed, one character a frame.
const TYPED: &str = "self.speed = self.speed * 0.7; // slow down";
/// How long the check phase idles after typing: twice the editor's settle time.
const IDLE: Duration = Duration::from_millis(600);

/// Writes the benchmark project into `dir`: `defs.qc` and `FILES` files of `LINES_PER_FILE`
/// lines of functions; the middle file's path.
fn write_project(dir: &Path) -> Result<PathBuf, String> {
    let io = |e: std::io::Error| format!("{}: {e}", dir.display());
    std::fs::create_dir_all(dir).map_err(io)?;
    let mut src = String::from("progs.dat\ndefs.qc\n");
    let defs = "entity self;\nfloat time;\n.float health;\n.float speed;\n\
                void(entity e) remove = #15;\n";
    std::fs::write(dir.join("defs.qc"), defs).map_err(io)?;
    for file in 0..FILES {
        let mut text = String::new();
        let mut function = 0_usize;
        while text.lines().count() < LINES_PER_FILE {
            writeln!(
                text,
                "void() f{file}_{function} =\n{{\n\tlocal float x;\n\tx = self.health + time;\n\
                 \tif (x > 10)\n\t\tself.speed = x * 0.5;\n\telse\n\t\tself.speed = 0;\n\
                 \tself.health = self.health - 1;\n}};\n"
            )
            .map_err(|e| e.to_string())?;
            function = function.saturating_add(1);
        }
        let name = format!("f{file}.qc");
        std::fs::write(dir.join(&name), text).map_err(io)?;
        src.push_str(&name);
        src.push('\n');
    }
    std::fs::write(dir.join("progs.src"), src).map_err(io)?;
    Ok(dir.join(format!("f{MIDDLE_FILE}.qc")))
}

/// A phase of the benchmark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Type,
    Check,
}

struct Bench {
    app: App,
    /// When the app was created, until its first frame ends.
    started: Option<Instant>,
    opened: Option<Duration>,
    frame: usize,
    /// When typing stopped.
    idle: Option<Instant>,
    samples: Vec<(Phase, u64)>,
    pending: Option<Phase>,
    out: PathBuf,
    host: String,
    result: Arc<Mutex<Option<bool>>>,
}

impl Bench {
    /// The phase of frame `frame` and the text it types, if any.
    fn step(&mut self) -> Option<(Phase, Option<char>)> {
        // Frame 0 opens; frame 1 gives the editor the keyboard.
        let at = self.frame.checked_sub(2)?;
        if let Some(c) = TYPED.chars().nth(at) {
            return Some((Phase::Type, Some(c)));
        }
        let idle = *self.idle.get_or_insert_with(Instant::now);
        (idle.elapsed() < IDLE).then_some((Phase::Check, None))
    }

    /// Writes the summary; whether the open and type budgets were met.
    fn finish(&self) -> Result<bool, String> {
        let opened = self.opened.unwrap_or(Duration::MAX);
        let mut summary = format!(
            "# Code editor budgets: {}\n\nA project of {} files of {} lines and `defs.qc`.\n\n",
            self.host, FILES, LINES_PER_FILE
        );
        let open_ok = opened <= OPEN;
        writeln!(
            summary,
            "Open the project and show a file: {:.0} ms, budget {} ms: {}.\n",
            opened.as_secs_f64() * 1000.0,
            OPEN.as_millis(),
            verdict(open_ok)
        )
        .map_err(|e| e.to_string())?;
        summary.push_str("| Phase | Frames | p50 ms | p99 ms | Max ms | Budget |\n");
        summary.push_str("| --- | ---: | ---: | ---: | ---: | --- |\n");
        let mut type_ok = false;
        for phase in [Phase::Type, Phase::Check] {
            let mut times: Vec<u64> = self
                .samples
                .iter()
                .filter(|(p, _)| *p == phase)
                .map(|(_, ns)| *ns)
                .collect();
            times.sort_unstable();
            let at = |pct: usize| {
                let rank = pct.saturating_mul(times.len()).div_ceil(100).max(1);
                times.get(rank.saturating_sub(1)).copied().unwrap_or(0)
            };
            let max = times.last().copied().unwrap_or(0);
            let ok = max <= FRAME_NS;
            if phase == Phase::Type {
                type_ok = ok && !times.is_empty();
            }
            writeln!(
                summary,
                "| {phase:?} | {} | {:.2} | {:.2} | {:.2} | every frame at most {:.2} ms: {} |",
                times.len(),
                ms(at(50)),
                ms(at(99)),
                ms(max),
                ms(FRAME_NS),
                verdict(ok)
            )
            .map_err(|e| e.to_string())?;
        }
        std::fs::create_dir_all(&self.out).map_err(|e| format!("{}: {e}", self.out.display()))?;
        let path = self.out.join(format!("{}-editor.md", self.host));
        std::fs::write(&path, &summary).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("{summary}");
        Ok(open_ok && type_ok)
    }
}

fn verdict(ok: bool) -> &'static str {
    if ok { "met" } else { "missed" }
}

/// Nanoseconds as milliseconds.
#[expect(
    clippy::cast_precision_loss,
    reason = "a frame time, far below 2^52 ns"
)]
fn ms(ns: u64) -> f64 {
    ns as f64 / 1_000_000.0
}

/// eframe's CPU time in seconds as nanoseconds.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a frame time in seconds, non-negative and far below u64::MAX ns"
)]
fn seconds_to_ns(seconds: f32) -> u64 {
    (f64::from(seconds.max(0.0)) * 1e9) as u64
}

impl eframe::App for Bench {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.pending = None;
        if let Some((phase, typed)) = self.step() {
            self.pending = Some(phase);
            if let Some(c) = typed {
                raw_input.events.push(egui::Event::Text(c.to_string()));
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if let (Some(phase), Some(cpu)) = (self.pending, frame.info().cpu_usage) {
            self.samples.push((phase, seconds_to_ns(cpu)));
        }
        if self.frame == 1 {
            self.app.shell().focus_editor(ui.ctx());
        }
        self.app.frame(ui);
        if let Some(started) = self.started.take() {
            self.opened = Some(started.elapsed());
        }
        self.frame = self.frame.saturating_add(1);
        if self.frame > 2 && self.pending.is_none() {
            let met = self.finish().unwrap_or_else(|e| {
                eprintln!("bench: {e}");
                false
            });
            if let Ok(mut result) = self.result.lock() {
                *result = Some(met);
            }
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ui.ctx().request_repaint();
    }
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(out) = args.next().map(PathBuf::from) else {
        eprintln!("usage: bench <out-dir> [host]");
        return ExitCode::from(2);
    };
    let host = args.next().unwrap_or_else(|| "local".to_owned());
    let dir = std::env::temp_dir().join(format!("quakec-studio-bench-{}", std::process::id()));
    let file = match write_project(&dir) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("bench: {error}");
            return ExitCode::FAILURE;
        }
    };
    let result = Arc::new(Mutex::new(None));
    let shared = Arc::clone(&result);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1440.0, 900.0]),
        ..Default::default()
    };
    let run = eframe::run_native(
        "quakec-studio-bench",
        options,
        Box::new(move |cc| {
            let started = Instant::now();
            let mut app = App::new(&cc.egui_ctx, None)?;
            let shell = app.shell_mut();
            shell.open_project(&dir)?;
            // The caret goes to the middle of the file.
            let text = std::fs::read_to_string(&file)?;
            let offset = text
                .match_indices('\n')
                .nth(MIDDLE_LINE)
                .map_or(0, |(at, _)| at.saturating_add(1));
            shell.execute("file.open", json!({ "path": file, "offset": offset }))?;
            Ok(Box::new(Bench {
                app,
                started: Some(started),
                opened: None,
                frame: 0,
                idle: None,
                samples: Vec::new(),
                pending: None,
                out,
                host,
                result: shared,
            }))
        }),
    );
    if let Err(error) = run {
        eprintln!("bench: {error}");
        return ExitCode::FAILURE;
    }
    match result.lock().ok().and_then(|r| *r) {
        Some(true) => ExitCode::SUCCESS,
        Some(false) | None => ExitCode::FAILURE,
    }
}
