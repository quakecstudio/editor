// SPDX-License-Identifier: GPL-3.0-or-later

//! The showcase screenshots (P7-14): the app rendered offscreen through wgpu, as the snapshot
//! harness renders it, on LibreQuake's maps compiled with full light and their own textures,
//! each scene a PNG for the website and the user guide.
//!
//! LibreQuake is not in this repository (corpus/manifest.toml), so the scenes are rendered from a
//! checkout of it whose `texture-wads/` holds its WADs built from their PNGs, as LibreQuake's
//! `compile_wads.py` builds them:
//!
//! ```text
//! cargo run --release -p quakec-studio --example showcase -- --librequake <dir> --out <dir>
//! ```

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use eframe::egui::{self, TexturesDelta};
use eframe::egui_wgpu::{self, ScreenDescriptor};
use egui_kittest::kittest::Queryable as _;
use egui_kittest::{Harness, TestRenderer};
use quakec_studio::App;
use quakec_studio::map_view::DEPTH_FORMAT;
use quakec_studio::strings::Text;
use qs_render::{Camera, Degrees, Offscreen};
use serde_json::{Value, json};

/// The window's size in points, the snapshot harness's.
const SIZE: [f32; 2] = [1440.0, 900.0];

/// The showcase's options.
#[derive(Parser)]
#[command(about = "Render the showcase screenshots from a LibreQuake checkout")]
struct Args {
    /// The LibreQuake checkout: its `lq1` game folder, `qcsrc` and built `texture-wads`.
    #[arg(long)]
    librequake: PathBuf,
    /// Where the PNGs are written.
    #[arg(long)]
    out: PathBuf,
}

/// egui_kittest's renderer as `qs_render::Offscreen`, with the depth buffer the window has.
struct DepthRenderer {
    offscreen: Offscreen,
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
        let pixels = [size.x.round(), size.y.round()].map(|v| v.max(0.0) as u32);
        let screen = ScreenDescriptor {
            pixels_per_point: ctx.pixels_per_point(),
            size_in_pixels: pixels,
        };
        let tessellated = ctx.tessellate(output.shapes.clone(), ctx.pixels_per_point());
        let rendered = self
            .offscreen
            .render(&tessellated, &screen)
            .map_err(|error| error.to_string())?;
        image::RgbaImage::from_raw(rendered.width, rendered.height, rendered.rgba)
            .ok_or_else(|| "no image".to_owned())
    }
}

/// One step of a scene.
enum Step {
    /// Run a command with its parameters.
    Run(&'static str, Value),
    /// Click the control named so.
    Click(Text),
    /// Look from here, yaw and pitch in degrees.
    Look([f32; 3], f32, f32),
}

/// A scene: its file name and its steps after the map opens.
struct Scene {
    name: &'static str,
    map: &'static str,
    steps: Vec<Step>,
}

/// The scenes, each from a LibreQuake map.
fn scenes() -> Vec<Scene> {
    let lit = || Step::Run("view.map_shading", json!({ "shading": "lit" }));
    let textured = || Step::Run("view.map_shading", json!({ "shading": "textured" }));
    vec![
        Scene {
            name: "showcase-lit",
            map: "e1/lq_e1m1.map",
            steps: vec![lit()],
        },
        Scene {
            name: "showcase-textured",
            map: "e1/lq_e1m1.map",
            steps: vec![textured()],
        },
        Scene {
            name: "showcase-quad",
            map: "e1/lq_e1m1.map",
            steps: vec![lit(), Step::Run("view.map_panes", json!({ "panes": "quad" }))],
        },
    ]
}

fn main() -> ExitCode {
    let args = Args::parse();
    if let Err(error) = run(&args) {
        tracing::error!(%error, "showcase");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// Renders every scene into `args.out`.
fn run(args: &Args) -> Result<(), String> {
    std::fs::create_dir_all(&args.out).map_err(|e| e.to_string())?;
    let game = args.librequake.join("lq1");
    for scene in scenes() {
        let map = game.join("maps/src").join(scene.map);
        let mut harness = harness()?;
        let app = harness.state_mut().as_mut().ok_or("no app")?;
        let shell = app.shell_mut();
        shell
            .execute("project.open", json!({ "path": game }))
            .map_err(|e| e.to_string())?;
        shell
            .execute("map.open", json!({ "path": map }))
            .map_err(|e| e.to_string())?;
        harness.run();
        compiled(&mut harness, &map)?;
        for step in &scene.steps {
            let app = harness.state_mut().as_mut().ok_or("no app")?;
            match step {
                Step::Run(id, params) => {
                    app.shell_mut()
                        .execute(id, params.clone())
                        .map_err(|e| format!("{id}: {e}"))?;
                }
                Step::Look(origin, yaw, pitch) => {
                    let mut camera = Camera::new(*origin, Degrees::new(*yaw));
                    camera.pitch = Degrees::new(*pitch);
                    app.shell_mut().set_map_camera(camera);
                }
                Step::Click(text) => harness.get_by_label(text.get()).click(),
            }
            harness.run();
        }
        let image = harness.render()?;
        let path = args.out.join(format!("{}.png", scene.name));
        image.save(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Compiles the open map with the Full profile, unless its BSP is newer than it.
fn compiled(harness: &mut Harness<'static, Option<App>>, map: &Path) -> Result<(), String> {
    let modified = |path: &Path| std::fs::metadata(path).and_then(|m| m.modified()).ok();
    if modified(&map.with_extension("bsp")) >= modified(map) && modified(map).is_some() {
        return Ok(());
    }
    let app = harness.state_mut().as_mut().ok_or("no app")?;
    app.shell_mut()
        .execute("map.choose_profile", json!({ "profile": "Full" }))
        .map_err(|e| e.to_string())?;
    harness.run();
    harness.get_by_label(Text::MapCompileProfile.get()).click();
    harness.run();
    harness.run();
    Ok(())
}

/// A harness running the app in a window of [`SIZE`] on a wgpu device it can draw maps on.
fn harness() -> Result<Harness<'static, Option<App>>, String> {
    let state = egui_kittest::wgpu::create_render_state(
        egui_kittest::wgpu::default_wgpu_setup(),
        egui_wgpu::RendererOptions {
            depth_stencil_format: Some(DEPTH_FORMAT),
            ..egui_wgpu::RendererOptions::PREDICTABLE
        },
    );
    let mut harness = Harness::builder()
        .with_size(SIZE)
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
    let mut app = App::checked_on_the_frame(&harness.ctx, None).map_err(|e| e.to_string())?;
    app.shell_mut().set_render_state(Some(state));
    *harness.state_mut() = Some(app);
    harness.run();
    Ok(harness)
}
