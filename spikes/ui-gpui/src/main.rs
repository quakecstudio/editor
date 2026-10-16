// SPDX-License-Identifier: GPL-3.0-or-later
//! gpui prototype for the UI framework spike (P0-10).
//!
//! A custom editor element (gpui ships no editor; Zed's lives in Zed's GPL `editor` crate),
//! a project panel and a wgpu 3D viewport, each its own view so that gpui re-renders only the
//! views that changed, as Zed does. `--bench OUT.csv` replays the shared script from
//! `on_next_frame` callbacks and records each frame's CPU time as the scripted action plus
//! gpui's own draw and present timings from its profiler.

mod editor;
mod viewport;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::profiler::{FrameEvent, FrameTimingCollector};
use gpui::{
    App, Bounds, Context, Entity, Focusable, Hsla, ImageSource, KeyBinding, RenderImage,
    StyleRefinement, Window, WindowBounds, WindowKind, WindowOptions, actions, div, img,
    prelude::*, px, rgb, size, uniform_list,
};
use ui_common::harness::{self, Args, Driver, Pending};
use ui_common::theme;

use crate::editor::{AddCursorBelow, Backspace, Editor, Newline, PageDown, PageUp, SingleCursor};

actions!(spike, [Quit]);

const VIEWPORT_W: f32 = 360.0;
/// How long the benchmark waits for a frame before nudging the window.
const WATCHDOG: Duration = Duration::from_millis(500);
const VIEWPORT_H: f32 = 300.0;

fn hsla(color: u32) -> Hsla {
    rgb(color).into()
}

fn fill_style() -> StyleRefinement {
    let mut style = StyleRefinement::default();
    style.size.width = Some(gpui::relative(1.).into());
    style.size.height = Some(gpui::relative(1.).into());
    style
}

fn main() -> ExitCode {
    let started = Instant::now();
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(usage) => {
            eprintln!("{usage}");
            return ExitCode::FAILURE;
        }
    };
    let opened = match harness::open(&args) {
        Ok(opened) => opened,
        Err(error) => {
            eprintln!("cannot open {}: {error}", args.project.display());
            return ExitCode::FAILURE;
        }
    };
    gpui_platform::application().run(move |cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("backspace", Backspace, Some("Editor")),
            KeyBinding::new("enter", Newline, Some("Editor")),
            KeyBinding::new("cmd-alt-down", AddCursorBelow, Some("Editor")),
            KeyBinding::new("ctrl-alt-down", AddCursorBelow, Some("Editor")),
            KeyBinding::new("escape", SingleCursor, Some("Editor")),
            KeyBinding::new("pagedown", PageDown, Some("Editor")),
            KeyBinding::new("pageup", PageUp, Some("Editor")),
            KeyBinding::new("cmd-q", Quit, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        let bounds = Bounds::centered(None, size(px(1440.0), px(900.0)), cx);
        // Benchmarks keep the window above others: an occluded window gets no frames.
        let kind = if args.bench.is_some() {
            WindowKind::PopUp
        } else {
            WindowKind::Normal
        };
        let opened_window = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                kind,
                ..WindowOptions::default()
            },
            move |window, cx| {
                window.set_window_title("QuakeC Studio spike - gpui");
                let workspace = cx.new(|cx| Workspace::new(opened, args, started, cx));
                let focus = workspace.read(cx).editor.focus_handle(cx);
                window.focus(&focus, cx);
                workspace
            },
        );
        match opened_window {
            Ok(handle) => {
                let _ = handle.update(cx, |workspace, window, cx| {
                    if workspace.bench.is_some() {
                        gpui::profiler::set_trace_enabled(true);
                        workspace.collector = Some(FrameTimingCollector::new());
                        schedule_tick(cx.entity(), window);
                        workspace.watchdog = Some(cx.spawn_in(window, async move |this, cx| {
                            loop {
                                cx.background_executor().timer(WATCHDOG).await;
                                let Ok(()) = this.update_in(cx, |ws, window, _| ws.nudge(window))
                                else {
                                    break;
                                };
                            }
                        }));
                    }
                    cx.activate(true);
                });
            }
            Err(error) => {
                eprintln!("cannot open a window: {error}");
                cx.quit();
            }
        }
    });
    ExitCode::SUCCESS
}

fn schedule_tick(workspace: Entity<Workspace>, window: &Window) {
    window.on_next_frame(move |window, cx| {
        workspace.update(cx, |ws, cx| ws.tick(window, cx));
    });
}

/// The window's root: lays out the panels and drives the benchmark. It is rendered once; the
/// panels re-render on their own.
struct Workspace {
    editor: Entity<Editor>,
    project: Entity<ProjectPanel>,
    side: Entity<SidePanel>,
    status: Entity<StatusBar>,
    bench: Option<(Driver, PathBuf)>,
    label: String,
    collector: Option<FrameTimingCollector>,
    pending: VecDeque<Pending>,
    finished: bool,
    last_tick: Instant,
    stalls: usize,
    watchdog: Option<gpui::Task<()>>,
}

impl Workspace {
    fn new(opened: harness::Opened, args: Args, started: Instant, cx: &mut Context<Self>) -> Self {
        let files = opened
            .project
            .files
            .iter()
            .map(|(path, lines)| (path.display().to_string(), *lines))
            .collect();
        let doc = opened.doc;
        let editor = cx.new(|cx| Editor::new(doc, cx));
        let project = cx.new(|_| ProjectPanel { files });
        let side =
            cx.new(|_| SidePanel::new(&args, opened.project.total_lines, opened.load, started));
        let status = cx.new(|cx| {
            cx.observe(&editor, |_, _, cx| cx.notify()).detach();
            StatusBar {
                editor: editor.clone(),
            }
        });
        Self {
            editor,
            project,
            side,
            status,
            label: args.label("gpui"),
            bench: args.bench.map(|out| (Driver::new(), out)),
            collector: None,
            pending: VecDeque::new(),
            finished: false,
            last_tick: Instant::now(),
            stalls: 0,
            watchdog: None,
        }
    }

    /// Restarts frame delivery if it stopped. On macOS gpui stops its display link when the
    /// window is occluded and restarts it on the next occlusion change; a window that misses
    /// that change gets no more frames. Re-activating it delivers one. Stalls are counted and
    /// reported; a stall is time between frames, never part of a frame's CPU time.
    fn nudge(&mut self, window: &mut Window) {
        if self.finished || self.last_tick.elapsed() < WATCHDOG {
            return;
        }
        self.stalls = self.stalls.saturating_add(1);
        window.activate_window();
        window.refresh();
    }

    /// Runs at the start of every frame in bench mode: records the frames gpui finished since
    /// the last tick, then applies the next scripted action.
    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.last_tick = Instant::now();
        // Every draw is a frame. gpui reports a present only for frames whose sample it considers
        // valid (it drops them across visibility changes), so a missing present counts as zero
        // rather than leaving the draw unmatched.
        let mut frames: Vec<(Duration, Duration)> = Vec::new();
        if let Some(collector) = &mut self.collector {
            for event in collector.collect_unseen() {
                match event {
                    FrameEvent::Draw(timing) => {
                        frames.push((timing.draw_duration(), Duration::ZERO))
                    }
                    FrameEvent::Present(timing) => {
                        if let Some(last) = frames.last_mut() {
                            last.1 = timing.present_duration();
                        }
                    }
                }
            }
        }
        let Some((driver, out)) = &mut self.bench else {
            return;
        };
        for (draw, present) in frames {
            if let Some(pending) = self.pending.pop_front() {
                let (draw, present) = (harness::nanos(draw), harness::nanos(present));
                let total = draw
                    .saturating_add(present)
                    .saturating_add(pending.sample.action_ns);
                driver.record_split(pending, total, draw, present);
            }
        }
        if !driver.finished() {
            let visible = self.editor.read(cx).visible_rows;
            let pending = self.editor.update(cx, |editor, cx| {
                cx.notify();
                driver.begin_frame(&mut editor.doc, visible)
            });
            if let Some(pending) = pending {
                self.pending.push_back(pending);
            }
        } else if !self.pending.is_empty() {
            // Keep drawing until every applied action's frame has been recorded.
            self.editor.update(cx, |_, cx| cx.notify());
        } else if !self.finished {
            self.finished = true;
            match driver.write(out, &self.label) {
                Ok(table) => {
                    let side = self.side.read(cx);
                    println!(
                        "{table}\nstartup: {}, first frame built {:.1} ms after launch\n\
                         frame delivery stalled {} times",
                        side.info,
                        side.first_frame.unwrap_or_default().as_secs_f64() * 1000.0,
                        self.stalls
                    );
                }
                Err(error) => eprintln!("cannot write {}: {error}", out.display()),
            }
            cx.quit();
            return;
        }
        schedule_tick(cx.entity(), window);
    }
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("workspace")
            .role(gpui::Role::Application)
            .aria_label("QuakeC Studio spike")
            .size_full()
            .flex()
            .flex_col()
            .bg(hsla(theme::SURFACE_PANEL))
            .text_color(hsla(theme::TEXT))
            .text_size(px(13.0))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_3()
                    .items_center()
                    .h(px(44.0))
                    .px_3()
                    .border_b_1()
                    .border_color(hsla(theme::SURFACE_RAISED))
                    .child("ember_mod")
                    .child(
                        div()
                            .px_3()
                            .py_1()
                            .rounded(px(5.0))
                            .bg(hsla(theme::ACCENT))
                            .text_color(hsla(0x16_1514))
                            .child("Play"),
                    )
                    .child("Pause")
                    .child("Step"),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .w(px(240.0))
                            .h_full()
                            .child(self.project.clone().cached(fill_style())),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(self.editor.clone().cached(fill_style())),
                    )
                    .child(
                        div()
                            .w(px(380.0))
                            .h_full()
                            .child(self.side.clone().cached(fill_style())),
                    ),
            )
            .child(
                div()
                    .h(px(24.0))
                    .child(self.status.clone().cached(fill_style())),
            )
    }
}

/// The project panel: every file, virtualised with `uniform_list`.
struct ProjectPanel {
    files: Vec<(String, usize)>,
}

impl Render for ProjectPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .p_2()
            .child("Project")
            .child(
                uniform_list(
                    "files",
                    self.files.len(),
                    cx.processor(|this, range: std::ops::Range<usize>, _, _| {
                        let mut items = Vec::with_capacity(range.len());
                        for (name, lines) in this.files.get(range).unwrap_or(&[]) {
                            items.push(
                                div()
                                    .h(px(24.0))
                                    .whitespace_nowrap()
                                    .overflow_hidden()
                                    .text_color(hsla(theme::TEXT_MUTED))
                                    .child(format!("{name}  ({lines})")),
                            );
                        }
                        items
                    }),
                )
                .flex_1(),
            )
    }
}

/// The right-hand panel: the 3D viewport, redrawn every frame, and the startup timings.
struct SidePanel {
    viewport: Option<viewport::Viewport>,
    viewport_error: Option<String>,
    image: Option<Arc<RenderImage>>,
    t0: Instant,
    info: String,
    started: Instant,
    first_frame: Option<Duration>,
}

impl SidePanel {
    fn new(args: &Args, total_lines: usize, load: Duration, started: Instant) -> Self {
        // The viewport renders at 2x for Retina; a real build would follow the window's scale.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "constants"
        )]
        let (w, h) = ((VIEWPORT_W * 2.0) as u32, (VIEWPORT_H * 2.0) as u32);
        let (viewport, viewport_error) = if args.viewport {
            match viewport::Viewport::new(w, h) {
                Ok(v) => (Some(v), None),
                Err(e) => (None, Some(e)),
            }
        } else {
            (None, None)
        };
        Self {
            viewport,
            viewport_error,
            image: None,
            t0: Instant::now(),
            info: format!(
                "{total_lines} lines open, loaded in {:.0} ms",
                load.as_secs_f64() * 1000.0
            ),
            started,
            first_frame: None,
        }
    }
}

impl Render for SidePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.first_frame.is_none() {
            self.first_frame = Some(self.started.elapsed());
        }
        if let Some(viewport) = &mut self.viewport {
            if let Some(image) = viewport.render(self.t0.elapsed().as_secs_f32())
                && let Some(old) = self.image.replace(image)
            {
                cx.drop_image(old, Some(window));
            }
            window.request_animation_frame();
        }
        let view = match &self.image {
            Some(image) => img(ImageSource::Render(image.clone()))
                .w(px(VIEWPORT_W))
                .h(px(VIEWPORT_H))
                .rounded(px(6.0))
                .into_any_element(),
            None => div()
                .w(px(VIEWPORT_W))
                .h(px(VIEWPORT_H))
                .rounded(px(6.0))
                .bg(hsla(theme::SURFACE_EDITOR))
                .into_any_element(),
        };
        let mut info = vec![self.info.clone()];
        if let Some(first) = self.first_frame {
            info.push(format!(
                "first frame {:.0} ms after launch",
                first.as_secs_f64() * 1000.0
            ));
        }
        if let Some(error) = &self.viewport_error {
            info.push(error.clone());
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .child("Viewport (wgpu)")
            .child(view)
            .children(info)
    }
}

/// The status bar: the primary cursor and the number of cursors. Re-renders when the editor
/// notifies.
struct StatusBar {
    editor: Entity<Editor>,
}

impl Render for StatusBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let doc = &self.editor.read(cx).doc;
        let primary = doc.primary();
        let status = format!(
            "Ln {}, Col {}   {} cursor(s)   {}",
            primary.line.saturating_add(1),
            primary.col.saturating_add(1),
            doc.cursors.len(),
            if doc.diff.is_some() {
                "Inline edit: review"
            } else {
                ""
            }
        );
        div()
            .size_full()
            .px_3()
            .flex()
            .items_center()
            .text_color(hsla(theme::TEXT_MUTED))
            .border_t_1()
            .border_color(hsla(theme::SURFACE_RAISED))
            .child(status)
    }
}
