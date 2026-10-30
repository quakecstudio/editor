// SPDX-License-Identifier: GPL-3.0-or-later

//! The editor view's frame-time benchmark (P2-27): the UI spike's scripted input
//! (`spikes/ui-common/src/script.rs`) replayed as egui events into an [`EditorView`] holding a
//! 50,000-line file, in an eframe window with the wgpu renderer, one step per frame.
//!
//! ```text
//! cargo run --release -p qs-editor-view --example bench -- <out-dir> [host]
//! ```
//!
//! Each frame's CPU time is eframe's `Frame::info().cpu_usage`, as the spike measured it: the
//! whole frame, UI, layout, tessellation and the wgpu submission, without the wait for vsync.
//! It writes `<host>-editor-view.csv` (phase, frame, CPU time in nanoseconds) and
//! `<host>-editor-view.md`, each phase's p50, p95, p99 and maximum, into `<out-dir>`, and exits
//! 1 when a measured phase's p99 is over one frame at 120 Hz. Keep the window on screen and the
//! display awake: an occluded window gets no frames on macOS.

#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "a benchmark binary that reports its summary"
)]

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use egui::{Event, FontData, FontDefinitions, FontFamily, Key, Modifiers, Vec2};
use qs_editor_view::{EditorView, Plain, ViewStyle};

const FONT: &[u8] = include_bytes!("../../../apps/desktop/assets/fonts/JetBrainsMono-Regular.ttf");

/// One frame at 120 Hz, in nanoseconds: the pass bar of ADR-0003.
const BUDGET_NS: u64 = 8_333_333;
/// The buffer's size, as in the spike.
const LINES: usize = 50_000;
/// The line the script starts editing at: the middle of the buffer.
const START_LINE: usize = 25_000;
/// The text the script types, as in the spike.
const TYPED: &str = "self.velocity = self.velocity * 0.7; // slow down";

/// A named run of frames; statistics are reported per phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Phase {
    Warmup,
    Type,
    Scroll,
    MultiCursor,
    Ime,
}

impl Phase {
    const ALL: [Self; 5] = [
        Self::Warmup,
        Self::Type,
        Self::Scroll,
        Self::MultiCursor,
        Self::Ime,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::Warmup => "warmup",
            Self::Type => "type",
            Self::Scroll => "scroll",
            Self::MultiCursor => "multicursor",
            Self::Ime => "ime",
        }
    }
}

/// One scripted step: what the view is to receive in one frame.
#[derive(Clone, Debug)]
enum Step {
    Idle,
    Goto(usize),
    Events(Vec<Event>),
}

fn key(key: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn wheel(lines: f32) -> Vec<Event> {
    vec![
        Event::PointerMoved(egui::Pos2::new(400.0, 300.0)),
        Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: Vec2::new(0.0, -lines),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
        },
    ]
}

/// The spike's script without its inline-diff phase, which is P2-18's: warm-up, typing at the
/// middle of the file, scrolling, typing at five carets, and IME composition and commits.
fn script() -> Vec<(Phase, Step)> {
    let mut steps = Vec::new();
    for _ in 0..120 {
        steps.push((Phase::Warmup, Step::Idle));
    }
    steps.push((Phase::Type, Step::Goto(START_LINE)));
    for _ in 0..12 {
        for c in TYPED.chars() {
            steps.push((Phase::Type, Step::Events(vec![Event::Text(c.to_string())])));
        }
        steps.push((
            Phase::Type,
            Step::Events(vec![key(Key::Enter, Modifiers::NONE)]),
        ));
    }
    for _ in 0..40 {
        steps.push((
            Phase::Type,
            Step::Events(vec![key(Key::Backspace, Modifiers::NONE)]),
        ));
    }
    for _ in 0..600 {
        steps.push((Phase::Scroll, Step::Events(wheel(3.0))));
    }
    for _ in 0..600 {
        steps.push((Phase::Scroll, Step::Events(wheel(-3.0))));
    }
    steps.push((
        Phase::MultiCursor,
        Step::Goto(START_LINE.saturating_add(400)),
    ));
    for _ in 0..4 {
        let add = key(Key::ArrowDown, Modifiers::COMMAND | Modifiers::ALT);
        steps.push((Phase::MultiCursor, Step::Events(vec![add])));
    }
    for _ in 0..5 {
        for c in TYPED.chars() {
            steps.push((
                Phase::MultiCursor,
                Step::Events(vec![Event::Text(c.to_string())]),
            ));
        }
        steps.push((
            Phase::MultiCursor,
            Step::Events(vec![key(Key::Enter, Modifiers::NONE)]),
        ));
    }
    steps.push((
        Phase::MultiCursor,
        Step::Events(vec![key(Key::Escape, Modifiers::NONE)]),
    ));
    for _ in 0..20 {
        for preedit in ["n", "に", "にh", "にほ", "にほn", "にほん"] {
            let event = Event::Ime(egui::ImeEvent::Preedit {
                text: preedit.to_owned(),
                active_range_chars: None,
            });
            steps.push((Phase::Ime, Step::Events(vec![event])));
        }
        let commit = Event::Ime(egui::ImeEvent::Commit("日本".to_owned()));
        steps.push((Phase::Ime, Step::Events(vec![commit])));
    }
    steps
}

struct Bench {
    view: EditorView,
    style: ViewStyle,
    steps: Vec<(Phase, Step)>,
    /// The next step to apply.
    next: usize,
    /// Each measured frame's phase and CPU time.
    samples: Vec<(Phase, u64)>,
    /// The phase of the frame whose time eframe reports next.
    pending: Option<Phase>,
    out: PathBuf,
    host: String,
    result: std::sync::Arc<std::sync::Mutex<Option<bool>>>,
}

impl Bench {
    /// Writes the CSV and the summary, and returns whether every measured phase passed.
    fn finish(&self) -> Result<bool, String> {
        let mut csv = String::from("phase,frame,cpu_ns\n");
        for (frame, (phase, ns)) in self.samples.iter().enumerate() {
            writeln!(csv, "{},{frame},{ns}", phase.name()).map_err(|e| e.to_string())?;
        }
        let mut summary = format!(
            "# Editor view frame times: {}\n\n| Phase | Frames | p50 ms | p95 ms | p99 ms | Max ms |\n\
             | --- | ---: | ---: | ---: | ---: | ---: |\n",
            self.host
        );
        let mut passed = true;
        for phase in Phase::ALL {
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
            let ms = |ns: u64| format!("{:.2}", ns_to_ms(ns));
            let p99 = at(99);
            if phase != Phase::Warmup && p99 > BUDGET_NS {
                passed = false;
            }
            writeln!(
                summary,
                "| {} | {} | {} | {} | {} | {} |",
                phase.name(),
                times.len(),
                ms(at(50)),
                ms(at(95)),
                ms(p99),
                ms(times.last().copied().unwrap_or(0))
            )
            .map_err(|e| e.to_string())?;
        }
        writeln!(
            summary,
            "\nThe bar is a p99 of at most {:.2} ms in every phase but the warm-up: {}.",
            ns_to_ms(BUDGET_NS),
            if passed { "passed" } else { "failed" }
        )
        .map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&self.out).map_err(|e| format!("{}: {e}", self.out.display()))?;
        let write = |name: String, text: &str| {
            let path = self.out.join(name);
            std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
        };
        write(format!("{}-editor-view.csv", self.host), &csv)?;
        write(format!("{}-editor-view.md", self.host), &summary)?;
        println!("{summary}");
        Ok(passed)
    }
}

/// Nanoseconds as milliseconds, for the summary.
#[expect(
    clippy::cast_precision_loss,
    reason = "a frame time, far below 2^52 ns"
)]
fn ns_to_ms(ns: u64) -> f64 {
    ns as f64 / 1_000_000.0
}

impl eframe::App for Bench {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        let Some((phase, step)) = self.steps.get(self.next).cloned() else {
            return;
        };
        self.next = self.next.saturating_add(1);
        self.pending = Some(phase);
        match step {
            Step::Idle => {}
            Step::Goto(line) => {
                let at = qs_editor_view::Position::new(line, 0);
                self.view
                    .state_mut()
                    .set_selections(&[qs_editor_view::Selection::caret(at)]);
                self.view.scroll_to(line.saturating_sub(10));
            }
            Step::Events(events) => raw_input.events.extend(events),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if let (Some(phase), Some(cpu)) = (self.pending.take(), frame.info().cpu_usage) {
            self.samples.push((phase, seconds_to_ns(cpu)));
        }
        let id = self.view.id();
        ui.memory_mut(|m| m.request_focus(id));
        egui::CentralPanel::default().show(ui, |ui| {
            self.view.show(ui, &self.style, &Plain);
        });
        if self.next >= self.steps.len() {
            let passed = self.finish().unwrap_or_else(|e| {
                eprintln!("bench: {e}");
                false
            });
            if let Ok(mut result) = self.result.lock() {
                *result = Some(passed);
            }
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ui.ctx().request_repaint();
    }
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

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(out) = args.next().map(PathBuf::from) else {
        eprintln!("usage: bench <out-dir> [host]");
        return ExitCode::from(2);
    };
    let host = args.next().unwrap_or_else(|| "local".to_owned());
    let text = (0..LINES)
        .map(|i| format!("float x{i} = {i}; // line {i} of the benchmark file"))
        .collect::<Vec<_>>()
        .join("\n");
    let result = std::sync::Arc::new(std::sync::Mutex::new(None));
    let app = Bench {
        view: EditorView::new(egui::Id::new("bench-editor"), &text),
        style: ViewStyle::default(),
        steps: script(),
        next: 0,
        samples: Vec::new(),
        pending: None,
        out,
        host,
        result: std::sync::Arc::clone(&result),
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]),
        ..eframe::NativeOptions::default()
    };
    let run = eframe::run_native(
        "qs-editor-view bench",
        options,
        Box::new(|cc| {
            let mut fonts = FontDefinitions::empty();
            fonts
                .font_data
                .insert("mono".to_owned(), FontData::from_static(FONT).into());
            for family in [FontFamily::Monospace, FontFamily::Proportional] {
                fonts.families.insert(family, vec!["mono".to_owned()]);
            }
            cc.egui_ctx.set_fonts(fonts);
            Ok(Box::new(app))
        }),
    );
    if let Err(e) = run {
        eprintln!("bench: {e}");
        return ExitCode::from(2);
    }
    match result.lock().ok().and_then(|r| *r) {
        Some(true) => ExitCode::SUCCESS,
        Some(false) => ExitCode::FAILURE,
        None => {
            eprintln!("bench: the window closed before the script ended");
            ExitCode::from(2)
        }
    }
}
