// SPDX-License-Identifier: GPL-3.0-or-later

//! The map view drawn in the app's window through its paint callback, compared with a golden
//! image (P5-05's acceptance), by a test renderer that gives egui's render pass the depth
//! buffer the app's window has.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::float_cmp,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or a mismatched snapshot is a failed test; sizes are \
              small whole pixels"
)]

use std::path::{Path, PathBuf};

use eframe::egui::{self, TexturesDelta};
use eframe::egui_wgpu::{self, ScreenDescriptor};
use egui_kittest::kittest::Queryable as _;
use egui_kittest::{Harness, OsThreshold, SnapshotOptions, TestRenderer};
use qs_render::{
    Batch, Camera, Degrees, Image, Look, MeshBuilder, Offscreen, Palette, Placement, Surface,
    Vertex,
};

use super::DEPTH_FORMAT;
use crate::App;
use crate::shell::CentreTab;
use crate::strings::Text;
use crate::tests::settle_playability;
use serde_json::json;

/// egui-kittest's test renderer as `qs_render::Offscreen`, with a depth buffer in its render
/// pass as the app's window has (`depth_buffer: 32`).
pub(crate) struct DepthRenderer {
    pub(crate) offscreen: Offscreen,
}

impl TestRenderer for DepthRenderer {
    fn handle_delta(&mut self, delta: &mut TexturesDelta) {
        self.offscreen.update_textures(delta);
    }

    fn render(
        &mut self,
        ctx: &egui::Context,
        output: &egui::FullOutput,
    ) -> Result<image::RgbaImage, String> {
        let size = ctx.content_rect().size() * ctx.pixels_per_point();
        let screen = ScreenDescriptor {
            pixels_per_point: ctx.pixels_per_point(),
            size_in_pixels: [size.x.round(), size.y.round()].map(|v| v as u32),
        };
        let tessellated = ctx.tessellate(output.shapes.clone(), ctx.pixels_per_point());
        let pixels = self
            .offscreen
            .render(&tessellated, &screen)
            .map_err(|error| error.to_string())?;
        image::RgbaImage::from_raw(pixels.width, pixels.height, pixels.rgba)
            .ok_or_else(|| "no image".to_owned())
    }
}

/// A floor of squares in a checker of two colours, 256 units across at z = 0, full
/// bright: the corner texel of the lightmap is always 255 (REND-1.6), in GLQuake's look so
/// the light is not doubled (REND-13.5).
fn checker_floor() -> qs_render::MapMesh {
    let palette = Palette::read(&[0; 768]).unwrap();
    let mut mesh = MeshBuilder::new(&palette, Look::GlQuake).finish();
    let corner = |x: f32, y: f32, s: f32, t: f32| Vertex {
        position: [x, y, 0.0],
        texture: [s, t],
        light: [0.0, 0.0],
    };
    mesh.vertices = vec![
        corner(-128.0, -128.0, 0.0, 0.0),
        corner(128.0, -128.0, 2.0, 0.0),
        corner(128.0, 128.0, 2.0, 2.0),
        corner(-128.0, 128.0, 0.0, 2.0),
    ];
    // Quake winds a face clockwise seen from its front (REND-1.10).
    mesh.indices = vec![0, 2, 1, 0, 3, 2];
    mesh.batches = vec![Batch {
        texture: 0,
        surface: Surface::Lit,
        first: 0,
        count: 6,
        model: 0,
    }];
    let ember = [238, 139, 69, 255];
    let stone = [70, 64, 60, 255];
    // 16 by 16 texels in blocks of 8, so filtering touches only the blocks' edges.
    let rgba: Vec<u8> = (0..16_u32)
        .flat_map(|t| {
            (0..16_u32).map(move |s| {
                if (s / 8 + t / 8) % 2 == 0 {
                    ember
                } else {
                    stone
                }
            })
        })
        .flatten()
        .collect();
    mesh.textures = vec![Image {
        width: 16,
        height: 16,
        rgba,
    }];
    mesh.skies = vec![None];
    mesh.animations = vec![None];
    mesh.models = vec![Placement::WORLD];
    mesh.mins = [-128.0, -128.0, 0.0];
    mesh.maxs = [128.0, 128.0, 0.0];
    mesh
}

fn goldens() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("snapshots")
}

// MAP-17: the map view draws the map in the window, through its paint callback on the window's
// device, as the golden image shows
#[test]
fn the_map_view_draws_in_the_window() {
    let state = egui_kittest::wgpu::create_render_state(
        egui_kittest::wgpu::default_wgpu_setup(),
        egui_wgpu::RendererOptions {
            depth_stencil_format: Some(DEPTH_FORMAT),
            ..egui_wgpu::RendererOptions::PREDICTABLE
        },
    );
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .with_pixels_per_point(1.0)
        .renderer(DepthRenderer {
            offscreen: Offscreen::from_state(state.clone(), DEPTH_FORMAT),
        })
        .build_ui_state(
            |ui, app: &mut Option<App>| {
                if let Some(app) = app {
                    app.frame(ui);
                }
            },
            None,
        );
    let mut app = App::checked_on_the_frame(&harness.ctx, None).unwrap();
    let mesh = checker_floor();
    let mut camera = Camera::new([-300.0, 0.0, 200.0], Degrees::new(0.0));
    camera.turn(Degrees::new(0.0), Degrees::new(30.0));
    assert!(
        !app.shell_mut().show_map(&mesh, camera),
        "no device until the window gives one"
    );
    app.shell_mut().set_render_state(Some(state));
    assert!(app.shell_mut().show_map(&mesh, camera));
    assert_eq!(app.shell().centre_tab(), CentreTab::Map);
    *harness.state_mut() = Some(app);
    harness.run();
    let options = SnapshotOptions::new()
        .output_path(goldens())
        .max_failed_pixels(OsThreshold::new(0).linux(400).windows(400));
    if let Err(error) = harness.try_snapshot_options("map-view", &options) {
        panic!("{error}");
    }
    let shown = harness
        .state()
        .as_ref()
        .unwrap()
        .shell()
        .map_view()
        .unwrap();
    assert_eq!(shown.camera().origin, [-300.0, 0.0, 200.0]);
}

/// One headless frame of `view` in an 800 by 600 window with `events`, at `time` seconds.
fn frame(view: &mut super::MapView, ctx: &egui::Context, time: f64, events: Vec<egui::Event>) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 600.0),
        )),
        time: Some(time),
        events,
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| {
        egui::CentralPanel::default().show(ui, |ui| view.ui(ui));
    });
    output.textures_delta.clear();
}

// MAP-17: dragging turns the view a quarter degree a point, the wheel moves it two map units
// a point, and the time runs from when it opened
#[test]
fn dragging_turns_and_the_wheel_moves_the_view() {
    let ctx = egui::Context::default();
    let mut view = super::MapView::new(Camera::new([0.0; 3], Degrees::new(90.0)));
    let at = egui::pos2(400.0, 300.0);
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    // A first frame lays the view out, so the press lands on it.
    frame(&mut view, &ctx, 1.0, vec![egui::Event::PointerMoved(at)]);
    frame(&mut view, &ctx, 1.01, vec![button(true)]);
    // Past the drag threshold first, then 40 points right and 20 down, a step a frame.
    let mut pointer = at + egui::vec2(10.0, 0.0);
    frame(
        &mut view,
        &ctx,
        1.05,
        vec![egui::Event::PointerMoved(pointer)],
    );
    let start = *view.camera();
    for step in 0..4 {
        pointer += egui::vec2(10.0, 5.0);
        let time = 1.1 + f64::from(step) * 0.01;
        frame(
            &mut view,
            &ctx,
            time,
            vec![egui::Event::PointerMoved(pointer)],
        );
    }
    frame(&mut view, &ctx, 1.2, vec![button(false)]);
    let camera = view.camera();
    assert!(
        (camera.yaw.get() - (start.yaw.get() - 10.0)).abs() < 1e-3,
        "40 points right turns 10 degrees right: {start:?} to {camera:?}"
    );
    assert!(
        (camera.pitch.get() - (start.pitch.get() + 5.0)).abs() < 1e-3,
        "20 points down looks 5 degrees down"
    );
    let before = camera.origin;
    frame(
        &mut view,
        &ctx,
        1.3,
        vec![egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            source: egui::MouseWheelSource::Unknown,
            delta: egui::vec2(0.0, 10.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    for t in 0..30 {
        frame(&mut view, &ctx, 1.4 + f64::from(t) * 0.1, Vec::new());
    }
    let after = view.camera().origin;
    let travelled = (0..3)
        .map(|i| (after[i] - before[i]).powi(2))
        .sum::<f32>()
        .sqrt();
    assert!(
        (travelled - 20.0).abs() < 0.5,
        "10 points of wheel, 2 units each: {travelled}"
    );
    assert!(after[1] != before[1], "forward, along the turned yaw");
    assert_eq!(
        view.view(1.0, 3.5).time,
        2.5,
        "seconds since the first frame"
    );
    assert_eq!(view.view(2.0, 1.0).eye, after);
    assert_ne!(
        view.view(2.0, 1.0).clip_from_world,
        view.view(1.0, 1.0).clip_from_world
    );
}

/// A copy of the keep fixture, with its WAD and palette, in a folder of its own for `name`.
fn keep_project(name: &str) -> PathBuf {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/keep");
    // Named `keep` inside a folder of its own, so the Project panel draws the same name each run.
    let dir = std::env::temp_dir()
        .join(format!("qs-shading-{name}-{}", std::process::id()))
        .join("keep");
    let _gone = std::fs::remove_dir_all(&dir);
    for sub in ["maps", "gfx"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
        for entry in std::fs::read_dir(fixture.join(sub)).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), dir.join(sub).join(entry.file_name())).unwrap();
        }
    }
    for file in ["progs.src", "defs.qc", "world.qc"] {
        std::fs::copy(fixture.join(file), dir.join(file)).unwrap();
    }
    dir
}

// MAP-17, P7-13: the map editor's 3D pane draws the keep textured, its faces shaded by the way
// they face, and lit by its compile's lightmaps, through the same renderer as the map view, as
// the golden images show; Lit says when there is no compile yet
#[test]
fn the_map_editor_draws_the_map_textured_and_lit() {
    let dir = keep_project("both");
    let state = egui_kittest::wgpu::create_render_state(
        egui_kittest::wgpu::default_wgpu_setup(),
        egui_wgpu::RendererOptions {
            depth_stencil_format: Some(DEPTH_FORMAT),
            ..egui_wgpu::RendererOptions::PREDICTABLE
        },
    );
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .with_pixels_per_point(1.0)
        .renderer(DepthRenderer {
            offscreen: Offscreen::from_state(state.clone(), DEPTH_FORMAT),
        })
        .build_ui_state(
            |ui, app: &mut Option<App>| {
                if let Some(app) = app {
                    app.frame(ui);
                }
            },
            None,
        );
    let mut app = App::checked_on_the_frame(&harness.ctx, None).unwrap();
    app.shell_mut().set_render_state(Some(state));
    let shell = app.shell_mut();
    shell
        .execute("project.open", json!({ "path": dir }))
        .unwrap();
    shell
        .execute("map.open", json!({ "path": dir.join("maps/keep.map") }))
        .unwrap();
    shell
        .execute("view.map_shading", json!({ "shading": "textured" }))
        .unwrap();
    *harness.state_mut() = Some(app);
    harness.run();
    let options = SnapshotOptions::new()
        .output_path(goldens())
        // Held exactly on macOS, where the goldens are made. Linux's and Windows' software
        // rasterizers filter textures a little differently, and these images are textured
        // across most of the pane, not a checker floor, so up to 6,000 pixels, half a percent of
        // the window, may differ there (2,498 on the Linux runner on 2026-11-15).
        .max_failed_pixels(OsThreshold::new(0).linux(6_000).windows(6_000));
    let mut failures = Vec::new();
    if let Err(error) = harness.try_snapshot_options("map-textured", &options) {
        failures.push(error.to_string());
    }
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell
        .execute("view.map_shading", json!({ "shading": "lit" }))
        .unwrap();
    harness.run();
    let note = |harness: &Harness<'static, Option<App>>| {
        harness
            .state()
            .as_ref()
            .unwrap()
            .shell()
            .map_editor()
            .shaded()
            .note()
    };
    assert_eq!(note(&harness), Some(Text::MapLitNone));
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell
        .execute("map.choose_profile", json!({ "profile": "Full" }))
        .unwrap();
    harness.run();
    harness.get_by_label(Text::MapCompileProfile.get()).click();
    settle_playability(&mut harness);
    assert!(dir.join("maps/keep.bsp").exists());
    assert_eq!(note(&harness), None, "the compile is the map's newest");
    // The compile's notice in Properties gives its time, which differs each run: the golden
    // image leaves the right panel out.
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell
        .execute("view.toggle_panel", json!({ "panel": "right" }))
        .unwrap();
    harness.run();
    // Every corner of the compiled keep has a corner of the textured view at its place with the
    // same texture coordinates: the editor lays textures out as qs-map-bsp writes them.
    let bytes = std::fs::read(dir.join("maps/keep.bsp")).unwrap();
    let bsp = qs_bsp::Bsp::read(&bytes, qs_bsp::BspLoader::FitzQuake).unwrap();
    let palette = Palette::read(&std::fs::read(dir.join("gfx/palette.lmp")).unwrap()).unwrap();
    let compiled = MeshBuilder::new(&palette, Look::QuakeSpasm);
    let mut compiled = compiled;
    compiled.add(&bsp, &[Placement::WORLD]).unwrap();
    let compiled = compiled.finish();
    let textured = harness
        .state()
        .as_ref()
        .unwrap()
        .shell()
        .map_editor()
        .textured_mesh();
    assert!(!compiled.vertices.is_empty());
    for corner in &compiled.vertices {
        assert!(
            textured
                .indices
                .as_chunks::<3>()
                .0
                .iter()
                .filter_map(|t| texture_at(&textured, t, corner.position))
                .any(|st| st
                    .iter()
                    .zip(corner.texture)
                    .all(|(a, b)| (a - b).abs() < 1e-3)),
            "{corner:?}: no textured triangle holds it with its texture coordinates"
        );
    }
    if let Err(error) = harness.try_snapshot_options("map-lit", &options) {
        failures.push(error.to_string());
    }
    // An edit after the compile makes it stale; with the BSP gone nothing is drawn; a BSP that
    // will not load is not drawn, and not read again until it is written again.
    let draws = |harness: &Harness<'static, Option<App>>| {
        harness
            .state()
            .as_ref()
            .unwrap()
            .shell()
            .map_editor()
            .shaded()
            .draws_faces()
    };
    assert!(draws(&harness));
    // A selection is not an edit: the compile is still the map's.
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell.execute("map.select", json!({ "entity": 2 })).unwrap();
    harness.run();
    assert_eq!(note(&harness), None);
    let shell = &mut harness.state_mut().as_mut().unwrap().shell;
    shell
        .execute(
            "map.set_key",
            json!({ "entity": 0, "key": "message", "value": "changed" }),
        )
        .unwrap();
    harness.run();
    assert_eq!(note(&harness), Some(Text::MapLitStale));
    std::fs::remove_file(dir.join("maps/keep.bsp")).unwrap();
    harness.run();
    assert_eq!(note(&harness), Some(Text::MapLitNone));
    assert!(!draws(&harness));
    std::fs::write(dir.join("maps/keep.bsp"), b"not a bsp").unwrap();
    harness.run();
    assert!(!draws(&harness));
    assert_eq!(note(&harness), Some(Text::MapLitNone));
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The texture coordinates the triangle `corners` of `mesh` gives the point `p`, when `p` lies
/// on it.
fn texture_at(mesh: &qs_render::MapMesh, corners: &[u32], p: [f32; 3]) -> Option<[f32; 2]> {
    let [a, b, c] = [0, 1, 2].map(|i| mesh.vertices[corners[i] as usize]);
    let sub = |x: [f32; 3], y: [f32; 3]| [x[0] - y[0], x[1] - y[1], x[2] - y[2]];
    let dot = |x: [f32; 3], y: [f32; 3]| x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
    let (u, v, w) = (
        sub(b.position, a.position),
        sub(c.position, a.position),
        sub(p, a.position),
    );
    let normal = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let length = dot(normal, normal).sqrt();
    if length == 0.0 || (dot(normal, w) / length).abs() > 0.01 {
        return None;
    }
    let (uu, uv, vv, wu, wv) = (dot(u, u), dot(u, v), dot(v, v), dot(w, u), dot(w, v));
    let d = uu * vv - uv * uv;
    let s = (vv * wu - uv * wv) / d;
    let t = (uu * wv - uv * wu) / d;
    if s < -1e-4 || t < -1e-4 || s + t > 1.0 + 1e-4 {
        return None;
    }
    Some([0, 1].map(|i| {
        a.texture[i] + s * (b.texture[i] - a.texture[i]) + t * (c.texture[i] - a.texture[i])
    }))
}
