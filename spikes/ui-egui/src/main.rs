// SPDX-License-Identifier: GPL-3.0-or-later
//! egui prototype for the UI framework spike (P0-10).
//!
//! A custom editor widget (egui's `TextEdit` lays out the whole buffer and has one cursor, so
//! it cannot pass the acceptance test), a project panel and a wgpu 3D viewport drawn through
//! an `egui_wgpu` paint callback. `--bench OUT.csv` replays the shared script and records each
//! frame's CPU time as eframe reports it (`Frame::info().cpu_usage`: the whole frame, UI,
//! tessellation and GPU submission, without the vsync wait).

use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Color32, CornerRadius, Event, EventFilter, FontFamily, FontId, Id, Key, Pos2, Rect,
    Sense, Stroke, TextFormat, Vec2, text::CCursor, text::LayoutJob,
};
use eframe::egui_wgpu::{self, wgpu};
use ui_common::document::{Document, Row, RowKind};
use ui_common::harness::{self, Args, Driver, Pending};
use ui_common::{scene, theme};

const ROW_HEIGHT: f32 = 20.0;
const FONT_SIZE: f32 = 13.0;
const GUTTER: f32 = 64.0;

fn color(rgb: u32) -> Color32 {
    let [r, g, b] = rgb.to_be_bytes()[1..4] else {
        return Color32::WHITE;
    };
    Color32::from_rgb(r, g, b)
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
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("QuakeC Studio spike - egui")
            .with_inner_size([1440.0, 900.0])
            // Benchmarks keep the window above others: an occluded window gets no frames.
            .with_window_level(if args.bench.is_some() {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            }),
        ..Default::default()
    };
    let result = eframe::run_native(
        "qs-spike-egui",
        options,
        Box::new(move |cc| {
            let render_state = cc
                .wgpu_render_state
                .as_ref()
                .ok_or("eframe did not start the wgpu renderer")?;
            install_cube(render_state);
            cc.egui_ctx.set_fonts(system_fonts()?);
            style(&cc.egui_ctx);
            Ok(Box::new(SpikeApp::new(opened, args, started)))
        }),
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// Font files to try, in order, for each family. egui has no system font lookup or fallback,
/// so the prototype names them; the CJK font is the fallback the IME test needs.
const MONO_FONTS: &[&str] = &[
    "/System/Library/Fonts/Menlo.ttc",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
    r"C:\Windows\Fonts\consola.ttf",
];
const UI_FONTS: &[&str] = &[
    "/System/Library/Fonts/SFNS.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    r"C:\Windows\Fonts\segoeui.ttf",
];
const CJK_FONTS: &[&str] = &[
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    r"C:\Windows\Fonts\msgothic.ttc",
];

fn first_font(candidates: &[&str]) -> Option<egui::FontData> {
    candidates
        .iter()
        .find_map(|path| std::fs::read(path).ok().map(egui::FontData::from_owned))
}

fn system_fonts() -> Result<egui::FontDefinitions, String> {
    let mut fonts = egui::FontDefinitions::empty();
    let mono = first_font(MONO_FONTS).ok_or("no monospace system font found")?;
    let ui = first_font(UI_FONTS).ok_or("no interface system font found")?;
    fonts.font_data.insert("mono".into(), Arc::new(mono));
    fonts.font_data.insert("ui".into(), Arc::new(ui));
    let mut mono_chain = vec!["mono".to_owned()];
    let mut ui_chain = vec!["ui".to_owned()];
    if let Some(cjk) = first_font(CJK_FONTS) {
        fonts.font_data.insert("cjk".into(), Arc::new(cjk));
        mono_chain.push("cjk".into());
        ui_chain.push("cjk".into());
    }
    fonts.families.insert(FontFamily::Monospace, mono_chain);
    fonts.families.insert(FontFamily::Proportional, ui_chain);
    Ok(fonts)
}

fn style(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::dark());
    ctx.global_style_mut(|style| {
        style.visuals.panel_fill = color(theme::SURFACE_PANEL);
        style.visuals.window_fill = color(theme::SURFACE_RAISED);
        style.visuals.extreme_bg_color = color(theme::SURFACE_EDITOR);
        style.visuals.override_text_color = Some(color(theme::TEXT));
        style.visuals.selection.bg_fill = color(theme::FOCUS).gamma_multiply(0.35);
    });
}

struct SpikeApp {
    doc: Document,
    files: Vec<(String, usize)>,
    total_lines: usize,
    load: Duration,
    started: Instant,
    first_frame: Option<Duration>,
    bench: Option<(Driver, std::path::PathBuf)>,
    label: String,
    show_viewport: bool,
    pending: Option<Pending>,
    finished: bool,
    visible_rows: usize,
    scroll_accum: f32,
    editor_id: Id,
    t0: Instant,
}

impl SpikeApp {
    fn new(opened: harness::Opened, args: Args, started: Instant) -> Self {
        let files = opened
            .project
            .files
            .iter()
            .map(|(path, lines)| (path.display().to_string(), *lines))
            .collect();
        Self {
            doc: opened.doc,
            files,
            total_lines: opened.project.total_lines,
            load: opened.load,
            started,
            first_frame: None,
            label: args.label("egui"),
            show_viewport: args.viewport,
            bench: args.bench.map(|out| (Driver::new(), out)),
            pending: None,
            finished: false,
            visible_rows: 40,
            scroll_accum: 0.0,
            editor_id: Id::new("editor"),
            t0: Instant::now(),
        }
    }

    fn drive(&mut self, ui: &egui::Ui, frame: &eframe::Frame) {
        let cpu = frame.info().cpu_usage;
        let Some((driver, out)) = &mut self.bench else {
            return;
        };
        if let (Some(pending), Some(cpu)) = (self.pending.take(), cpu) {
            driver.record(
                pending,
                harness::nanos(Duration::from_secs_f32(cpu.max(0.0))),
            );
        }
        if let Some(pending) = driver.begin_frame(&mut self.doc, self.visible_rows) {
            self.pending = Some(pending);
            ui.ctx().request_repaint();
        } else if !self.finished {
            self.finished = true;
            match driver.write(out, &self.label) {
                Ok(table) => println!(
                    "{table}\nstartup: {} lines loaded in {:.1} ms, first frame built {:.1} ms after launch",
                    self.total_lines,
                    self.load.as_secs_f64() * 1000.0,
                    self.first_frame.unwrap_or_default().as_secs_f64() * 1000.0
                ),
                Err(error) => eprintln!("cannot write {}: {error}", out.display()),
            }
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn handle_input(&mut self, ui: &egui::Ui, response: &egui::Response, rect: Rect, char_w: f32) {
        let id = self.editor_id;
        if response.clicked() || self.first_frame.is_none() {
            ui.memory_mut(|m| m.request_focus(id));
        }
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            );
        });
        if let Some(pos) = response.interact_pointer_pos()
            && response.clicked()
        {
            let row = ((pos.y - rect.top()) / ROW_HEIGHT).max(0.0);
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "clamped"
            )]
            let row = self.doc.scroll_row.saturating_add(row as usize);
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "clamped"
            )]
            let col_chars = ((pos.x - rect.left() - GUTTER) / char_w).round().max(0.0) as usize;
            let line = row.min(self.doc.line_count().saturating_sub(1));
            let text = self.doc.line(line);
            let col = text
                .char_indices()
                .nth(col_chars)
                .map_or(text.len(), |(i, _)| i);
            let cursor = ui_common::document::Cursor { line, col };
            if ui.input(|i| i.modifiers.alt) {
                if !self.doc.cursors.iter().any(|c| c.line == line) {
                    self.doc.cursors.push(cursor);
                    self.doc.cursors.sort();
                }
            } else {
                self.doc.cursors = vec![cursor];
            }
        }
        if response.hovered() {
            self.scroll_accum -= ui.input(|i| i.smooth_scroll_delta.y) / ROW_HEIGHT;
            #[expect(clippy::cast_possible_truncation, reason = "a few rows per frame")]
            let whole = self.scroll_accum.trunc() as isize;
            if whole != 0 {
                self.doc.scroll_by(whole);
                #[expect(clippy::cast_precision_loss, reason = "a few rows per frame")]
                let used = whole as f32;
                self.scroll_accum -= used;
            }
        }
        if !ui.memory(|m| m.owns_ime_events(id)) {
            return;
        }
        let events = ui.input(|i| i.events.clone());
        let mut edited = false;
        for event in events {
            match event {
                Event::Text(text) if !text.chars().any(char::is_control) => {
                    self.doc.insert(&text);
                    edited = true;
                }
                Event::Ime(egui::ImeEvent::Preedit { text, .. }) => {
                    self.doc.marked = (!text.is_empty()).then_some(text);
                }
                Event::Ime(egui::ImeEvent::Commit(text)) => {
                    if text.is_empty() {
                        self.doc.marked = None;
                    } else {
                        self.doc.insert(&text);
                        edited = true;
                    }
                }
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => match key {
                    Key::Enter => {
                        self.doc.newline();
                        edited = true;
                    }
                    Key::Backspace => {
                        self.doc.backspace();
                        edited = true;
                    }
                    Key::ArrowDown if modifiers.alt && modifiers.command => {
                        self.doc.add_cursor_below();
                        edited = true;
                    }
                    Key::Escape => self.doc.single_cursor(),
                    Key::PageDown => self
                        .doc
                        .scroll_by(isize::try_from(self.visible_rows).unwrap_or(1)),
                    Key::PageUp => self
                        .doc
                        .scroll_by(-isize::try_from(self.visible_rows).unwrap_or(1)),
                    _ => {}
                },
                _ => {}
            }
        }
        if edited {
            ui_common::script::reveal(&mut self.doc, self.visible_rows);
        }
    }

    fn editor(&mut self, ui: &mut egui::Ui) {
        let rect = ui.available_rect_before_wrap();
        let response = ui.interact(rect, self.editor_id, Sense::click());
        let font = FontId::new(FONT_SIZE, FontFamily::Monospace);
        let char_w = ui.fonts_mut(|f| f.glyph_width(&font, 'M'));
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "screen rows"
        )]
        let visible = (rect.height() / ROW_HEIGHT).ceil().max(1.0) as usize;
        self.visible_rows = visible;
        self.handle_input(ui, &response, rect, char_w);

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::ZERO, color(theme::SURFACE_EDITOR));
        let rows = self.doc.rows(self.doc.scroll_row, visible);
        let primary = self.doc.primary();
        let mut ime_cursor = None;
        for (i, row) in rows.iter().enumerate() {
            #[expect(clippy::cast_precision_loss, reason = "screen rows")]
            let top = rect.top() + i as f32 * ROW_HEIGHT;
            self.paint_row(
                &painter,
                ui,
                row,
                rect,
                top,
                &font,
                primary,
                &mut ime_cursor,
            );
        }
        if let Some(cursor_rect) = ime_cursor
            && ui.memory(|m| m.owns_ime_events(self.editor_id))
        {
            ui.output_mut(|o| {
                o.ime = Some(egui::output::IMEOutput {
                    purpose: egui::IMEPurpose::Normal,
                    rect,
                    cursor_rect,
                    should_interrupt_composition: false,
                });
            });
        }
        self.accessibility(ui, &rows, rect);
    }

    /// Exposes the editor as a multi-line text input with one text run per visible row. egui
    /// parents AccessKit nodes by `Ui`, so each run is a child `Ui` parented to the editor.
    fn accessibility(&self, ui: &mut egui::Ui, rows: &[Row], rect: Rect) {
        use egui::accesskit;
        let id = self.editor_id;
        let (runs, caret) = ui_common::a11y::text_runs(rows, self.doc.primary());
        let mut run_ids = Vec::with_capacity(runs.len());
        for (i, run) in runs.into_iter().enumerate() {
            #[expect(clippy::cast_precision_loss, reason = "screen rows")]
            let top = rect.top() + i as f32 * ROW_HEIGHT;
            let row_rect = Rect::from_min_size(
                Pos2::new(rect.left() + GUTTER, top),
                Vec2::new(rect.width() - GUTTER, ROW_HEIGHT),
            );
            let child = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt(("a11y-row", i))
                    .max_rect(row_rect)
                    .accessibility_parent(id),
            );
            let run_id = child.unique_id();
            run_ids.push(run_id);
            ui.ctx().accesskit_node_builder(run_id, |node| {
                node.set_role(accesskit::Role::TextRun);
                node.set_value(run.value);
                node.set_character_lengths(run.character_lengths);
                node.set_bounds(accesskit::Rect {
                    x0: row_rect.min.x.into(),
                    y0: row_rect.min.y.into(),
                    x1: row_rect.max.x.into(),
                    y1: row_rect.max.y.into(),
                });
            });
        }
        ui.ctx().accesskit_node_builder(id, |node| {
            node.set_role(accesskit::Role::MultilineTextInput);
            node.set_label("QuakeC source");
            if let Some((row, character_index)) = caret
                && let Some(run_id) = run_ids.get(row)
            {
                let at = accesskit::TextPosition {
                    node: run_id.accesskit_id(),
                    character_index,
                };
                node.set_text_selection(accesskit::TextSelection {
                    anchor: at,
                    focus: at,
                });
            }
        });
    }

    #[expect(clippy::too_many_arguments, reason = "spike code")]
    fn paint_row(
        &self,
        painter: &egui::Painter,
        ui: &egui::Ui,
        row: &Row,
        rect: Rect,
        top: f32,
        font: &FontId,
        primary: ui_common::document::Cursor,
        ime_cursor: &mut Option<Rect>,
    ) {
        let row_rect = Rect::from_min_size(
            Pos2::new(rect.left(), top),
            Vec2::new(rect.width(), ROW_HEIGHT),
        );
        if let Some(bg) = theme::row_background(row.kind) {
            painter.rect_filled(row_rect, CornerRadius::ZERO, color(bg));
        } else if row.line == Some(primary.line) {
            painter.rect_filled(
                row_rect,
                CornerRadius::ZERO,
                color(theme::SELECTION_LINE_BG),
            );
        }
        let gutter_text = match (row.kind, row.number) {
            (RowKind::Removed, _) => "-".to_owned(),
            (RowKind::Added, _) => "+".to_owned(),
            (RowKind::Normal, Some(n)) => n.to_string(),
            (RowKind::Normal, None) => String::new(),
        };
        let baseline = top + (ROW_HEIGHT - FONT_SIZE) / 2.0;
        painter.text(
            Pos2::new(rect.left() + GUTTER - 16.0, baseline),
            egui::Align2::RIGHT_TOP,
            gutter_text,
            font.clone(),
            color(theme::TEXT_FAINT),
        );
        let marked = match (&self.doc.marked, row.line) {
            (Some(text), Some(line)) if line == primary.line => Some((primary.col, text.as_str())),
            _ => None,
        };
        let (text, runs) = row.display(marked);
        let mut job = LayoutJob::default();
        for run in &runs {
            let format = TextFormat {
                font_id: font.clone(),
                color: color(run.color),
                italics: run.italic,
                underline: if run.underline {
                    Stroke::new(1.0, color(theme::TEXT))
                } else {
                    Stroke::NONE
                },
                ..TextFormat::default()
            };
            job.append(text.get(run.range.clone()).unwrap_or(""), 0.0, format);
        }
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        let origin = Pos2::new(rect.left() + GUTTER, baseline);
        let Some(line) = row.line else {
            painter.galley(origin, galley, color(theme::TEXT));
            return;
        };
        for cursor in self.doc.cursors.iter().filter(|c| c.line == line) {
            let mut byte = cursor.col;
            if *cursor == primary
                && let Some((_, m)) = marked
            {
                byte = byte.saturating_add(m.len());
            }
            let chars = text.get(..byte).map_or(0, |s| s.chars().count());
            let pos = galley.pos_from_cursor(CCursor::new(chars));
            let caret = Rect::from_min_size(
                Pos2::new(origin.x + pos.min.x, top + 2.0),
                Vec2::new(2.0, ROW_HEIGHT - 4.0),
            );
            painter.rect_filled(caret, CornerRadius::ZERO, color(theme::ACCENT));
            if *cursor == primary {
                *ime_cursor = Some(caret);
            }
        }
        painter.galley(origin, galley, color(theme::TEXT));
    }

    fn viewport(&self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("Viewport (wgpu)").strong());
        let size = Vec2::new(ui.available_width(), 300.0);
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let t = self.t0.elapsed().as_secs_f32();
        let aspect = rect.width() / rect.height().max(1.0);
        ui.painter()
            .rect_filled(rect, CornerRadius::same(6), color(theme::SURFACE_EDITOR));
        if self.show_viewport {
            ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                rect,
                CubeCallback {
                    mvp: scene::mvp(t, aspect),
                },
            ));
        }
        ui.add_space(8.0);
        ui.label(format!(
            "{} lines open, loaded in {:.0} ms",
            self.total_lines,
            self.load.as_secs_f64() * 1000.0
        ));
        if let Some(first) = self.first_frame {
            ui.label(format!(
                "first frame {:.0} ms after launch",
                first.as_secs_f64() * 1000.0
            ));
        }
    }
}

impl eframe::App for SpikeApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.drive(ui, frame);
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("ember_mod").strong());
                let play = egui::Button::new(egui::RichText::new("Play").color(Color32::BLACK))
                    .fill(color(theme::ACCENT));
                ui.add(play);
                ui.label("Pause");
                ui.label("Step");
            });
        });
        let primary = self.doc.primary();
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "Ln {}, Col {}   {} cursor(s)   {}",
                    primary.line.saturating_add(1),
                    primary.col.saturating_add(1),
                    self.doc.cursors.len(),
                    if self.doc.diff.is_some() {
                        "Inline edit: review"
                    } else {
                        ""
                    }
                ));
            });
        });
        egui::Panel::left("project")
            .default_size(240.0)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Project").strong());
                egui::ScrollArea::vertical().show_rows(ui, 24.0, self.files.len(), |ui, range| {
                    for (name, lines) in self.files.get(range).unwrap_or(&[]) {
                        ui.add_sized(
                            [ui.available_width(), 24.0],
                            egui::Label::new(format!("{name}  ({lines})")).truncate(),
                        );
                    }
                });
            });
        egui::Panel::right("inspector")
            .default_size(380.0)
            .show(ui, |ui| self.viewport(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.editor(ui));
        if self.first_frame.is_none() {
            self.first_frame = Some(self.started.elapsed());
        }
        ui.ctx().request_repaint();
    }
}

/// GPU resources for the cube, stored in egui-wgpu's callback resources.
struct CubeResources {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    count: u32,
}

fn install_cube(render_state: &egui_wgpu::RenderState) {
    let device = &render_state.device;
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("cube"),
        source: wgpu::ShaderSource::Wgsl(scene::SHADER.into()),
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("cube"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("cube"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("cube"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: 24,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3],
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(render_state.target_format.into())],
        }),
        primitive: wgpu::PrimitiveState {
            cull_mode: Some(wgpu::Face::Back),
            ..wgpu::PrimitiveState::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });
    let cube = scene::cube();
    let vertices = wgpu::util::DeviceExt::create_buffer_init(
        device,
        &wgpu::util::BufferInitDescriptor {
            label: Some("cube"),
            contents: &scene::vertex_bytes(&cube),
            usage: wgpu::BufferUsages::VERTEX,
        },
    );
    let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("cube"),
        size: 64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("cube"),
        layout: &layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniforms.as_entire_binding(),
        }],
    });
    render_state
        .renderer
        .write()
        .callback_resources
        .insert(Arc::new(CubeResources {
            pipeline,
            vertices,
            uniforms,
            bind_group,
            count: u32::try_from(cube.len()).unwrap_or(0),
        }));
}

struct CubeCallback {
    mvp: scene::Mat4,
}

impl egui_wgpu::CallbackTrait for CubeCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(cube) = resources.get::<Arc<CubeResources>>() {
            queue.write_buffer(&cube.uniforms, 0, &scene::mat_bytes(&self.mvp));
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        if let Some(cube) = resources.get::<Arc<CubeResources>>() {
            pass.set_pipeline(&cube.pipeline);
            pass.set_bind_group(0, &cube.bind_group, &[]);
            pass.set_vertex_buffer(0, cube.vertices.slice(..));
            pass.draw(0..cube.count, 0..1);
        }
    }
}
